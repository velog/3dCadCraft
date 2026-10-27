//! Placement dimensions of the Hole command (`docs/spec/2.3-hole.md`
//! "Placement"): the distances of the centres to model edges, both the
//! ones being made (`HoleDraft::dims`, face clicks of this command) and the
//! ones already in the hole's sketch (Edit Feature: `PointLineDistance`
//! dimensions of its Center Points, kept on a working copy of that sketch
//! until OK). In the view they are edited like sketch dimensions — a
//! double-click on the value edits it in place, the right button offers
//! Edit / Redefine (the next edge click replaces the edge) / Delete — and
//! every centre gets two arrows that drag it: along a dimension the arrow
//! changes the value, a centre with fewer than two dimensions moves freely
//! along the face.

use eframe::egui::{self, Color32, Stroke};

use super::CadCraftApp;
use super::hole::HoleDraft;
use super::manip;
use crate::kernel::{Point3, Real, Vector3};
use crate::part::{HoleCenter, PartDocument};
use crate::sketch::{Constraint, ConstraintId, PointId, Sketch};

/// The hole's own sketch while Edit Feature is open: a copy the dimension
/// edits go to, written into the part at OK (`rev` keys the preview).
#[derive(Clone, Debug)]
pub(super) struct SketchCopy {
    /// Index of the sketch in the part.
    pub index: usize,
    /// The working copy.
    pub sketch: Sketch,
    /// Bumped on every edit.
    pub rev: u32,
}

/// One placement dimension.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum DimRef {
    /// `HoleDraft::dims[j]` (a centre clicked in this command).
    Draft(usize),
    /// A dimension of the hole's sketch (the working copy).
    Sketch(ConstraintId),
}

/// A centre that can be dragged.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum CentreRef {
    /// `HoleDraft::points[k]`.
    Draft(usize),
    /// A Center Point of the hole's sketch.
    Sketch(PointId),
}

/// What an arrow drags.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum HoleHandle {
    /// The value of a dimension.
    Dim(DimRef),
    /// The centre itself, along a direction of the face.
    Free(CentreRef),
}

/// A dimension as shown: centre, foot on the edge's line, edge ends.
#[derive(Clone, Debug)]
pub(super) struct DimView {
    pub r: DimRef,
    pub centre: CentreRef,
    pub p: Point3,
    pub foot: Point3,
    pub ends: [Point3; 2],
    pub value: Real,
    pub expr: Option<String>,
    /// A reference (driven) dimension of the sketch: shown in brackets,
    /// not edited or dragged.
    pub driven: bool,
}

/// The foot of the perpendicular from `p` on the line through `a`, `b`.
fn foot(p: Point3, [a, b]: [Point3; 2]) -> Point3 {
    let d = b - a;
    let t = (p - a).dot(&d) / d.norm_squared().max(1e-300);
    a + d * t
}

impl HoleDraft {
    /// Every placement dimension (this command's, then the sketch's).
    pub(super) fn dim_views(&self) -> Vec<DimView> {
        let mut out = Vec::new();
        for (j, x) in self.dims.iter().enumerate() {
            let Some(&p) = self.points.get(x.center) else {
                continue;
            };
            out.push(DimView {
                r: DimRef::Draft(j),
                centre: CentreRef::Draft(x.center),
                p,
                foot: x.foot(p),
                ends: x.ends,
                value: x.value,
                // A link shows as the dimension it follows (`= #1`).
                expr: x.link.map(|k| format!("= #{}", k + 1)).or(x.expr.clone()),
                driven: false,
            });
        }
        let Some(c) = &self.sk else { return out };
        let sk = &c.sketch;
        let centres = self.sketch_centres();
        for (id, k) in sk.constraints() {
            let &Constraint::PointLineDistance { point, line, value } = k else {
                continue;
            };
            if !centres.contains(&point) {
                continue;
            }
            let (Some(pt), Ok((a, b))) = (sk.point(point), sk.line_points(line)) else {
                continue;
            };
            let (Some(pa), Some(pb)) = (sk.point(a), sk.point(b)) else {
                continue;
            };
            let p = sk.to_world(&pt.position);
            let ends = [sk.to_world(&pa.position), sk.to_world(&pb.position)];
            let driven = sk.is_driven(id);
            out.push(DimView {
                r: DimRef::Sketch(id),
                centre: CentreRef::Sketch(point),
                p,
                foot: foot(p, ends),
                ends,
                // A reference dimension shows what the geometry measures.
                value: if driven {
                    sk.measure(id).unwrap_or(value)
                } else {
                    value
                },
                expr: sk.dim_param(id).expr.clone(),
                driven,
            });
        }
        out
    }

    /// The Center Points of the hole's sketch this hole uses.
    fn sketch_centres(&self) -> Vec<PointId> {
        self.def
            .centers
            .iter()
            .filter_map(|c| match c {
                HoleCenter::Point(p) => Some(*p),
                _ => None,
            })
            .collect()
    }

    /// Sets a dimension's value (and equation); the centre follows.
    pub(super) fn set_dim_value(&mut self, r: DimRef, value: Real, expr: Option<String>) {
        match r {
            DimRef::Draft(j) => self.set_dim(j, value, expr),
            DimRef::Sketch(id) => {
                let Some(c) = self.sk.as_mut() else { return };
                let mut sk = c.sketch.clone();
                if sk.set_dimension(id, value).is_err() {
                    return;
                }
                if let Some(dp) = sk.dim_param_mut(id) {
                    dp.expr = expr;
                }
                // A failed solve leaves the copy as it was (rule 5).
                if sk.solve().is_ok() {
                    c.sketch = sk;
                    c.rev += 1;
                }
            }
        }
    }

    /// Removes a dimension (the centre stays where it is).
    pub(super) fn delete_dim(&mut self, r: DimRef) {
        match r {
            DimRef::Draft(j) => {
                if j < self.dims.len() {
                    self.dims.remove(j);
                    // Links to it go; later indices shift.
                    for x in &mut self.dims {
                        x.link = match x.link {
                            Some(k) if k == j => None,
                            Some(k) if k > j => Some(k - 1),
                            other => other,
                        };
                    }
                }
            }
            DimRef::Sketch(id) => {
                if let Some(c) = self.sk.as_mut()
                    && c.sketch.remove_constraint(id).is_ok()
                {
                    c.rev += 1;
                }
            }
        }
        self.redefine = None;
    }

    /// `target` follows `source` (a click on `source` while `target` is
    /// edited in place): `d2 = d1`. A sketch dimension takes the source's
    /// parameter name as its equation.
    pub(super) fn link_dims(&mut self, target: DimRef, source: DimRef) -> Result<(), String> {
        if target == source {
            return Ok(());
        }
        let views = self.dim_views();
        let value = views
            .iter()
            .find(|v| v.r == source)
            .map(|v| v.value)
            .ok_or("that dimension is gone")?;
        let sketch_name = |s: &Self, id: ConstraintId| {
            s.sk.as_ref()
                .map(|c| c.sketch.dim_param(id).name)
                .filter(|n| !n.is_empty())
        };
        match (target, source) {
            (DimRef::Draft(t), DimRef::Draft(s)) => {
                // No loops: the source must not follow the target.
                if self.dims.get(s).and_then(|x| x.link) == Some(t) {
                    return Err("these dimensions would follow each other".into());
                }
                self.set_dim(t, value, None);
                if let Some(x) = self.dims.get_mut(t) {
                    x.link = Some(s);
                }
            }
            (DimRef::Draft(t), DimRef::Sketch(id)) => {
                let name = sketch_name(self, id).ok_or("that dimension has no name yet")?;
                self.set_dim(t, value, Some(name));
            }
            (DimRef::Sketch(t), DimRef::Sketch(id)) => {
                let name = sketch_name(self, id).ok_or("that dimension has no name yet")?;
                self.set_dim_value(DimRef::Sketch(t), value, Some(name));
            }
            (DimRef::Sketch(_), DimRef::Draft(_)) => {
                return Err("click a dimension that is already in the hole's sketch".into());
            }
        }
        Ok(())
    }

    /// Moves a centre to `at` (model point on its face).
    pub(super) fn move_centre(&mut self, c: CentreRef, at: Point3) {
        match c {
            CentreRef::Draft(k) => {
                if let Some(p) = self.points.get_mut(k) {
                    *p = at;
                }
                // The dimensions of a moved centre follow its new place.
                for x in self.dims.iter_mut().filter(|x| x.center == k) {
                    x.value = (at - x.foot(at)).norm();
                }
            }
            CentreRef::Sketch(id) => {
                let Some(copy) = self.sk.as_mut() else { return };
                let mut sk = copy.sketch.clone();
                let q = sk.plane.project(&at);
                if sk.move_point(id, q).is_ok() && sk.solve().is_ok() {
                    copy.sketch = sk;
                    copy.rev += 1;
                }
            }
        }
    }

    /// The centres with their model point and face normal.
    fn centres(&self, part: &PartDocument) -> Vec<(CentreRef, Point3, Vector3)> {
        let mut out = Vec::new();
        if let Some(n) = self.face.as_deref().and_then(|f| face_normal(part, f)) {
            for (k, p) in self.points.iter().enumerate() {
                out.push((CentreRef::Draft(k), *p, n));
            }
        }
        if let Some(c) = &self.sk {
            let n = c.sketch.plane.normal.into_inner();
            for id in self.sketch_centres() {
                if let Some(pt) = c.sketch.point(id) {
                    out.push((CentreRef::Sketch(id), c.sketch.to_world(&pt.position), n));
                }
            }
        }
        out
    }

    /// The arrows: for each centre two directions on its face — along its
    /// dimensions (the value) and, for the missing ones, free moves.
    pub(super) fn handles(
        &self,
        part: &PartDocument,
        size: &dyn Fn(Point3, Vector3) -> Real,
    ) -> Vec<(HoleHandle, manip::Handle)> {
        let views = self.dim_views();
        let mut out = Vec::new();
        let centres = self.centres(part);
        for (c, p, n) in centres.iter().copied() {
            // A fixed screen length, shorter than half the way to the
            // nearest other centre (arrows of close holes must not cross).
            let near = centres
                .iter()
                .filter(|(o, ..)| *o != c)
                .map(|(_, q, _)| (q - p).norm())
                .fold(Real::INFINITY, Real::min);
            let len = size(p, n).min(0.45 * near).max(1e-3);
            let mut dirs: Vec<Vector3> = Vec::new();
            for v in views.iter().filter(|v| v.centre == c && !v.driven) {
                let Some(dir) = (v.p - v.foot).try_normalize(1e-12) else {
                    continue;
                };
                if dirs.iter().any(|d| d.cross(&dir).norm() < 1e-3) || dirs.len() >= 2 {
                    continue;
                }
                dirs.push(dir);
                out.push((
                    HoleHandle::Dim(v.r),
                    manip::Handle {
                        field: "hole_dim",
                        path: vec![p, p + dir * len],
                        grip: p + dir * len,
                        geom: manip::DragGeom::Linear {
                            origin: v.foot,
                            dir,
                            scale: 1.0,
                        },
                        arrow: Some(dir),
                        angle: false,
                        signed: false,
                    },
                ));
            }
            // Free directions for what the dimensions leave open.
            let any = if n.x.abs() < 0.9 {
                Vector3::x()
            } else {
                Vector3::y()
            };
            let first = dirs
                .first()
                .map(|d| n.cross(d))
                .unwrap_or_else(|| (any - n * any.dot(&n)).normalize());
            let mut free = vec![first];
            if dirs.is_empty() {
                free.push(n.cross(&first));
            }
            for dir in free.into_iter().take(2 - dirs.len().min(2)) {
                out.push((
                    HoleHandle::Free(c),
                    manip::Handle {
                        field: "hole_move",
                        path: vec![p, p + dir * len],
                        grip: p + dir * len,
                        // The value is the offset from where the drag began
                        // (the handle is cloned at the press).
                        geom: manip::DragGeom::Linear {
                            origin: p + dir * len,
                            dir,
                            scale: 1.0,
                        },
                        arrow: Some(dir),
                        angle: false,
                        signed: true,
                    },
                ));
            }
        }
        out
    }
}

/// The normal of a planar face by name.
fn face_normal(part: &PartDocument, name: &str) -> Option<Vector3> {
    let f = part.body.find_face(name)?;
    match part.body.face_surface_of(f)? {
        crate::kernel::Surface::Plane(p) => Some(p.normal.into_inner()),
        _ => None,
    }
}

impl CadCraftApp {
    /// Edit Feature of a hole: its sketch's dimensions become editable
    /// here (a working copy, written into the part at OK).
    pub(super) fn hole_load_sketch(&mut self) {
        let Some(d) = self.hole_draft.as_ref() else {
            return;
        };
        if d.edit.is_none() || d.sk.is_some() || d.def.sketch.is_empty() {
            return;
        }
        let id = d.def.sketch.clone();
        let Some(index) = self.part().sketches.iter().position(|s| s.id == id) else {
            return;
        };
        // Every dimension has its parameter name (`d2 = d1` links by it).
        self.part_mut().sync_parameters();
        let sketch = self.part().sketches[index].sketch.clone();
        if let Some(d) = self.hole_draft.as_mut() {
            d.sk = Some(SketchCopy {
                index,
                sketch,
                rev: 0,
            });
        }
    }

    /// The arrows of the open Hole command.
    pub(super) fn hole_handles(&self) -> Vec<(HoleHandle, manip::Handle)> {
        let Some(d) = &self.hole_draft else {
            return Vec::new();
        };
        // About 40 px long whatever the zoom: mm per pixel at the centre,
        // measured along the face (the larger of two directions on it).
        let rect = self.viewport_rect;
        let size = |p: Point3, n: Vector3| {
            let any = if n.x.abs() < 0.9 {
                Vector3::x()
            } else {
                Vector3::y()
            };
            let u = n.cross(&any).normalize();
            let v = n.cross(&u);
            let Some(o) = self.camera.project(&p, rect) else {
                return 1.0;
            };
            let px = [u, v]
                .iter()
                .filter_map(|w| self.camera.project(&(p + w), rect))
                .map(|q| q.distance(o) as Real)
                .fold(0.0, Real::max);
            40.0 / px.max(1e-6)
        };
        d.handles(self.part(), &size)
    }

    /// A drag of a Hole arrow: `h` is the handle as it was at the press.
    pub(super) fn hole_manip(&mut self, h: &manip::Handle, v: Real) {
        let target = match self.hole_drag {
            Some(t) => t,
            None => {
                // Which arrow: the one whose grip is where this one was.
                let Some(t) = self
                    .hole_handles()
                    .into_iter()
                    .find(|(_, x)| (x.grip - h.grip).norm() < 1e-9)
                    .map(|(t, _)| t)
                else {
                    return;
                };
                self.hole_drag = Some(t);
                t
            }
        };
        let manip::DragGeom::Linear { origin, dir, .. } = h.geom else {
            return;
        };
        let Some(d) = self.hole_draft.as_mut() else {
            return;
        };
        match target {
            HoleHandle::Dim(r) => d.set_dim_value(r, v.max(1e-3), None),
            HoleHandle::Free(c) => {
                // From the centre at the press (grip minus the shaft).
                let len = h.path.first().map_or(0.0, |p| (h.grip - p).norm());
                let start = origin - dir * len;
                d.move_centre(c, start + dir * v);
            }
        }
    }

    /// The dimension whose value is under `pos`.
    pub(super) fn hole_dim_at(&self, pos: egui::Pos2, rect: egui::Rect) -> Option<DimRef> {
        let d = self.hole_draft.as_ref()?;
        d.dim_views()
            .into_iter()
            .filter(|v| !v.driven)
            .filter_map(|v| {
                let at = self.hole_dim_label(&v, rect)?;
                let r = egui::Rect::from_center_size(at, egui::vec2(56.0, 18.0));
                r.contains(pos).then(|| (v.r, at.distance(pos)))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(r, _)| r)
    }

    /// Screen place of a dimension's value.
    fn hole_dim_label(&self, v: &DimView, rect: egui::Rect) -> Option<egui::Pos2> {
        let a = self.camera.project(&v.p, rect)?;
        let b = self.camera.project(&v.foot, rect)?;
        Some(egui::pos2((a.x + b.x) / 2.0, (a.y + b.y) / 2.0) - egui::vec2(0.0, 14.0))
    }

    /// The dimensions in the view: the edge, the perpendicular, the value
    /// (the one being redefined or edited highlighted) and the arrows.
    pub(super) fn draw_hole_dims(&self, painter: &egui::Painter, rect: egui::Rect) {
        let Some(d) = &self.hole_draft else { return };
        let blue = Color32::from_rgb(20, 90, 200);
        let hot = Color32::from_rgb(230, 120, 20);
        for v in d.dim_views() {
            let col = if d.redefine == Some(v.r) {
                hot
            } else if v.driven {
                Color32::from_gray(120)
            } else {
                blue
            };
            let (Some(a), Some(b)) = (
                self.camera.project(&v.p, rect),
                self.camera.project(&v.foot, rect),
            ) else {
                continue;
            };
            if let (Some(e0), Some(e1)) = (
                self.camera.project(&v.ends[0], rect),
                self.camera.project(&v.ends[1], rect),
            ) {
                painter.line_segment([e0, e1], Stroke::new(3.0, col));
            }
            painter.arrow(b, a - b, Stroke::new(1.5, col));
            if d.inplace.as_ref().is_some_and(|(r, ..)| *r == v.r) {
                continue; // the edit box is there
            }
            let mut text = match &v.expr {
                Some(e) if e.starts_with('=') => {
                    format!("{e} ({})", crate::part::hole::num(v.value, 3))
                }
                Some(e) => format!("{e} = {}", crate::part::hole::num(v.value, 3)),
                None => crate::part::hole::num(v.value, 3),
            };
            // This command's dimensions are numbered as in the panel.
            if let DimRef::Draft(j) = v.r {
                text = format!("#{}: {text}", j + 1);
            }
            // A reference dimension in brackets (Inventor).
            if v.driven {
                text = format!("({text})");
            }
            if let Some(at) = self.hole_dim_label(&v, rect) {
                let g = painter.layout_no_wrap(text, egui::FontId::proportional(13.0), col);
                let r = egui::Rect::from_center_size(at, g.size() + egui::vec2(6.0, 2.0));
                painter.rect_filled(r, 2.0, Color32::from_rgba_unmultiplied(255, 255, 255, 200));
                painter.galley(r.min + egui::vec2(3.0, 1.0), g, col);
            }
        }
    }

    /// Double-click (edit in place) and right-click (menu) on a value.
    pub(super) fn hole_dim_input(
        &mut self,
        response: &egui::Response,
        pointer: Option<egui::Pos2>,
        rect: egui::Rect,
    ) -> bool {
        let Some(p) = pointer else { return false };
        let Some(r) = self.hole_dim_at(p, rect) else {
            return false;
        };
        if response.double_clicked_by(egui::PointerButton::Primary) {
            let v = self.hole_draft.as_ref().and_then(|d| {
                d.dim_views()
                    .into_iter()
                    .find(|v| v.r == r)
                    .map(|v| v.expr.unwrap_or_else(|| crate::part::hole::num(v.value, 3)))
            });
            if let (Some(d), Some(text)) = (self.hole_draft.as_mut(), v) {
                d.inplace = Some((r, text, true));
            }
            return true;
        }
        if response.secondary_clicked() {
            if let Some(d) = self.hole_draft.as_mut() {
                d.dim_menu = Some((r, p));
            }
            return true;
        }
        // A click on another dimension while one is edited: it follows it.
        if response.clicked_by(egui::PointerButton::Primary)
            && let Some(d) = self.hole_draft.as_mut()
            && let Some((target, ..)) = d.inplace.clone()
            && target != r
        {
            match d.link_dims(target, r) {
                Ok(()) => {
                    d.inplace = None;
                    self.file_status = None;
                }
                Err(e) => self.file_status = Some(Err(format!("Hole: {e}"))),
            }
            return true;
        }
        // A plain click on a value is not a pick of what lies below it.
        response.clicked_by(egui::PointerButton::Primary)
    }

    /// The in-place edit box and the dimension's right-click menu.
    pub(super) fn hole_dim_windows(&mut self, ctx: &egui::Context, rect: egui::Rect) {
        let Some(d) = self.hole_draft.as_ref() else {
            return;
        };
        // ---- the right-click menu
        if let Some((r, at)) = d.dim_menu {
            let mut close = false;
            let mut action = None;
            let area = egui::Area::new(egui::Id::new("hole_dim_menu"))
                .fixed_pos(at)
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        for (label, a) in [
                            ("Edit Dimension", 0),
                            ("Redefine (pick another edge)", 1),
                            ("Delete", 2),
                        ] {
                            if ui.button(label).clicked() {
                                action = Some(a);
                            }
                        }
                    });
                });
            if ctx.input(|i| i.pointer.any_pressed())
                && ctx
                    .input(|i| i.pointer.interact_pos())
                    .is_some_and(|p| !area.response.rect.contains(p))
            {
                close = true;
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
            if let Some(a) = action {
                close = true;
                let views = self
                    .hole_draft
                    .as_ref()
                    .map(|d| d.dim_views())
                    .unwrap_or_default();
                let cur = views.iter().find(|v| v.r == r).cloned();
                if a == 2 {
                    self.checkpoint("Hole: Delete Dimension");
                }
                if let Some(d) = self.hole_draft.as_mut() {
                    match a {
                        0 => {
                            if let Some(v) = cur {
                                let t =
                                    v.expr.unwrap_or_else(|| crate::part::hole::num(v.value, 3));
                                d.inplace = Some((r, t, true));
                            }
                        }
                        1 => d.redefine = Some(r),
                        _ => d.delete_dim(r),
                    }
                }
            }
            if close && let Some(d) = self.hole_draft.as_mut() {
                d.dim_menu = None;
            }
        }
        // ---- the in-place edit box
        let Some(d) = self.hole_draft.as_ref() else {
            return;
        };
        let Some((r, _, _)) = d.inplace.clone() else {
            return;
        };
        let Some(v) = d.dim_views().into_iter().find(|v| v.r == r) else {
            if let Some(d) = self.hole_draft.as_mut() {
                d.inplace = None;
            }
            return;
        };
        let Some(at) = self.hole_dim_label(&v, rect) else {
            return;
        };
        let table = self.part().parameter_table();
        // The same field as everywhere (rule 13): expressions, the `>` menu
        // with List Parameters, Recently Used and Measure.
        let id = egui::Id::new("hole_dim_inplace");
        let mut commit: Option<String> = None;
        let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        egui::Area::new(egui::Id::new("hole_dim_inplace_area"))
            .fixed_pos(at - egui::vec2(70.0, 12.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let spec = super::valuefield::FieldSpec {
                            id,
                            value: v.value,
                            unit: crate::part::expr::Unit::Mm,
                            name: None,
                            expr: v.expr.as_deref(),
                            table: &table,
                        };
                        commit = super::valuefield::value_field(ui, &spec);
                        if let Some(d) = self.hole_draft.as_mut()
                            && let Some((_, _, focus)) = d.inplace.as_mut()
                            && std::mem::take(focus)
                        {
                            ui.memory_mut(|m| m.request_focus(id.with("te")));
                        }
                        close |= ui.small_button("x").on_hover_text("Close (Esc)").clicked();
                    });
                });
            });
        if close && commit.is_none() {
            if let Some(d) = self.hole_draft.as_mut() {
                d.inplace = None;
            }
            return;
        }
        let Some(text) = commit else { return };
        if let Some(d) = self.hole_draft.as_mut() {
            d.inplace = None;
        }
        match crate::part::params::eval_field_in(&table, &text, crate::part::expr::Unit::Mm, None) {
            Ok((_, value, _)) if value > 0.0 => {
                let expr = (!crate::part::expr::is_plain_number(&text)).then_some(text);
                if let Some(d) = self.hole_draft.as_mut() {
                    d.set_dim_value(r, value, expr);
                }
                self.file_status = None;
            }
            Ok(_) => self.file_status = Some(Err("Hole: the distance must be positive".into())),
            Err(e) => self.file_status = Some(Err(format!("Hole: {e}"))),
        }
    }

    /// Redefine: the edge clicked replaces the dimension's edge (the value
    /// becomes the centre's distance to it).
    pub(super) fn hole_redefine(&mut self, r: DimRef, a: String, b: String, ends: [Point3; 2]) {
        let Some(d) = self.hole_draft.as_mut() else {
            return;
        };
        d.redefine = None;
        match r {
            DimRef::Draft(j) => {
                let Some(x) = d.dims.get_mut(j) else { return };
                let Some(&p) = d.points.get(x.center) else {
                    return;
                };
                x.a = a;
                x.b = b;
                x.ends = ends;
                x.value = (p - x.foot(p)).norm();
                x.expr = None;
            }
            DimRef::Sketch(id) => {
                let Some(c) = d.sk.clone() else { return };
                let Some(&Constraint::PointLineDistance { point, .. }) = c.sketch.constraint(id)
                else {
                    return;
                };
                // Project the new edge through a copy of the part (its
                // reference line, associative by vertex names).
                let mut trial = self.part().clone();
                let Some(ps) = trial.sketches.get_mut(c.index) else {
                    return;
                };
                ps.sketch = c.sketch.clone();
                let line = match trial.project_segment(c.index, &a, &b) {
                    Ok(l) => l,
                    Err(e) => {
                        self.file_status = Some(Err(format!("Hole: {e}")));
                        return;
                    }
                };
                let mut sk = trial.sketches[c.index].sketch.clone();
                let _ = sk.remove_constraint(id);
                let Some(pt) = sk.point(point).map(|p| sk.to_world(&p.position)) else {
                    return;
                };
                let value = (pt - foot(pt, ends)).norm().max(1e-3);
                if sk
                    .add_constraint(Constraint::PointLineDistance { point, line, value })
                    .is_err()
                {
                    self.file_status =
                        Some(Err("Hole: the new edge over-constrains the centre".into()));
                    return;
                }
                if let Some(d) = self.hole_draft.as_mut()
                    && let Some(copy) = d.sk.as_mut()
                {
                    copy.sketch = sk;
                    copy.rev += 1;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gui::hole::HoleDim;
    use crate::part::Feature;

    fn vertex(part: &PartDocument, n: &str) -> Point3 {
        part.body
            .vertices()
            .find(|(_, v)| v.name.as_deref() == Some(n))
            .map(|(_, v)| v.point)
            .unwrap_or_else(|| panic!("vertex {n}"))
    }

    /// A box with one hole at (12, 9) dimensioned to the front (y 0) and
    /// left (x 0) top edges.
    fn holed() -> (PartDocument, HoleDraft) {
        let part = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
        let mut d = HoleDraft::new();
        d.face = Some("Box1:Top".into());
        d.points = vec![Point3::new(12.0, 9.0, 20.0)];
        for (a, b) in [("Box1:V001", "Box1:V101"), ("Box1:V001", "Box1:V011")] {
            let ends = [vertex(&part, a), vertex(&part, b)];
            let mut x = HoleDim {
                center: 0,
                a: a.into(),
                b: b.into(),
                ends,
                value: 0.0,
                expr: None,
                link: None,
            };
            x.value = (d.points[0] - x.foot(d.points[0])).norm();
            d.dims.push(x);
        }
        (part, d)
    }

    /// A reference (driven) dimension of the hole's sketch is shown with
    /// what the geometry measures, flagged so it is drawn in brackets and
    /// neither edited nor dragged.
    #[test]
    fn reference_dimensions_are_flagged() {
        let (mut part, d) = holed();
        let def = d.build(&mut part).unwrap();
        part.add_hole(def).unwrap();
        let k = part.last_inserted();
        let Some(Feature::Hole(h)) = part.features.get(k).cloned() else {
            panic!("no hole");
        };
        let mut e = HoleDraft::editing(h, k);
        let index = part
            .sketches
            .iter()
            .position(|s| s.id == e.def.sketch)
            .unwrap();
        let mut sketch = part.sketches[index].sketch.clone();
        let id = e
            .def
            .centers
            .iter()
            .find_map(|c| match c {
                HoleCenter::Point(p) => Some(*p),
                _ => None,
            })
            .and_then(|p| {
                sketch.constraints().find_map(|(id, c)| {
                    matches!(c, Constraint::PointLineDistance { point, .. } if *point == p)
                        .then_some(id)
                })
            })
            .unwrap();
        sketch.set_driven(id, true).unwrap();
        e.sk = Some(SketchCopy {
            index,
            sketch,
            rev: 0,
        });
        let views = e.dim_views();
        let v = views.iter().find(|v| v.r == DimRef::Sketch(id)).unwrap();
        assert!(v.driven);
        assert!(
            (v.value - 9.0).abs() < 1e-9 || (v.value - 12.0).abs() < 1e-9,
            "{}",
            v.value
        );
        assert_eq!(views.iter().filter(|v| !v.driven).count(), 1);
    }

    /// Edit Feature: the hole's two dimensions come back from its sketch,
    /// a new value moves the hole, Delete removes one.
    #[test]
    fn sketch_dimensions_come_back_and_edit() {
        let (mut part, d) = holed();
        let def = d.build(&mut part).unwrap();
        part.add_hole(def).unwrap();
        let k = part.last_inserted();
        let Some(Feature::Hole(h)) = part.features.get(k).cloned() else {
            panic!("no hole");
        };
        let mut e = HoleDraft::editing(h, k);
        let index = part
            .sketches
            .iter()
            .position(|s| s.id == e.def.sketch)
            .unwrap();
        e.sk = Some(SketchCopy {
            index,
            sketch: part.sketches[index].sketch.clone(),
            rev: 0,
        });
        let views = e.dim_views();
        assert_eq!(views.len(), 2, "both dimensions of the centre");
        let mut values: Vec<Real> = views.iter().map(|v| v.value).collect();
        values.sort_by(Real::total_cmp);
        assert!((values[0] - 9.0).abs() < 1e-9 && (values[1] - 12.0).abs() < 1e-9);
        // 9 -> 15 from the front edge.
        let front = views
            .iter()
            .find(|v| (v.value - 9.0).abs() < 1e-9)
            .unwrap()
            .r;
        e.set_dim_value(front, 15.0, None);
        part.sketches[index].sketch = e.sk.as_ref().unwrap().sketch.clone();
        part.replace_feature(k, Feature::Hole(e.def.clone()))
            .unwrap();
        assert!((part.holes()[0].origin.y - 15.0).abs() < 1e-6);
        assert!((part.holes()[0].origin.x - 12.0).abs() < 1e-6);
        // Arrows: one along each dimension, no free ones.
        let hs = e.handles(&part, &|_, _| 5.0);
        assert_eq!(hs.len(), 2);
        assert!(hs.iter().all(|(t, _)| matches!(t, HoleHandle::Dim(_))));
        e.delete_dim(front);
        assert_eq!(e.dim_views().len(), 1);
        // One dimension left: one free arrow along the edge it keeps.
        let hs = e.handles(&part, &|_, _| 5.0);
        assert_eq!(hs.len(), 2);
        assert_eq!(
            hs.iter()
                .filter(|(t, _)| matches!(t, HoleHandle::Free(_)))
                .count(),
            1
        );
    }

    /// `d2 = d1`: one dimension follows another of the same command and
    /// keeps following at OK (its equation is the other's name).
    #[test]
    fn a_dimension_follows_another() {
        let (mut part, mut d) = holed();
        d.link_dims(DimRef::Draft(1), DimRef::Draft(0)).unwrap();
        assert!((d.dims[1].value - 9.0).abs() < 1e-9);
        assert!(
            d.link_dims(DimRef::Draft(0), DimRef::Draft(1)).is_err(),
            "no loop"
        );
        d.set_dim(0, 6.0, None);
        assert!((d.dims[1].value - 6.0).abs() < 1e-9, "the follower moved");
        assert!((d.points[0].x - 6.0).abs() < 1e-9 && (d.points[0].y - 6.0).abs() < 1e-9);
        let def = d.build(&mut part).unwrap();
        part.add_hole(def).unwrap();
        let sk = &part.sketches[0].sketch;
        let ids: Vec<_> = sk
            .constraints()
            .filter(|(_, c)| matches!(c, Constraint::PointLineDistance { .. }))
            .map(|(id, _)| id)
            .collect();
        let (n0, e1) = (sk.dim_param(ids[0]).name, sk.dim_param(ids[1]).expr);
        assert_eq!(e1.as_deref(), Some(n0.as_str()));
        // The source changed through its parameter: the hole stays on the
        // diagonal.
        part.set_parameter_expr(&n0, "11").unwrap();
        part.regenerate().unwrap();
        let o = part.holes()[0].origin;
        assert!(
            (o.x - 11.0).abs() < 1e-6 && (o.y - 11.0).abs() < 1e-6,
            "{o:?}"
        );
    }

    /// A clicked centre without dimensions has two free arrows; moving it
    /// keeps it on the face; a dimension's value follows the move.
    #[test]
    fn free_arrows_and_moves_of_a_clicked_centre() {
        let (part, mut d) = holed();
        let keep = d.dims.clone();
        d.dims.clear();
        let hs = d.handles(&part, &|_, _| 5.0);
        assert_eq!(hs.len(), 2);
        let (a, b) = (&hs[0].1, &hs[1].1);
        let (Some(u), Some(v)) = (a.arrow, b.arrow) else {
            panic!("arrows");
        };
        assert!(u.dot(&v).abs() < 1e-9 && u.z.abs() < 1e-9 && v.z.abs() < 1e-9);
        d.dims = keep;
        d.move_centre(CentreRef::Draft(0), Point3::new(20.0, 5.0, 20.0));
        let mut values: Vec<Real> = d.dim_views().iter().map(|v| v.value).collect();
        values.sort_by(Real::total_cmp);
        assert!((values[0] - 5.0).abs() < 1e-9 && (values[1] - 20.0).abs() < 1e-9);
    }
}
