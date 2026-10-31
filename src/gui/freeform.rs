//! **Freeform** (`docs/spec/5.5-freeform.md`): Edit Form of a Freeform feature — its control
//! cage drawn over the sheet, points picked in the view and dragged in the view plane (Alias's
//! default way of moving CVs) with a smooth falloff to their neighbours, or set by numbers. Every
//! move rebuilds the feature at once; a drag is one Undo step.

use eframe::egui::{self, Color32, PointerButton, Pos2, Rect, RichText, Stroke};

use super::CadCraftApp;
use super::icons::Icon;
use super::panels::{plabel, psection};
use super::valuefield::ExprValue;
use crate::kernel::{Plane, Point3, Real, Vector3};
use crate::part::freeform::FreeformOp;
use crate::part::{BodyOp, Feature};

/// How near (pixels) a click must be to a cage point.
const PICK_PX: f32 = 8.0;

/// The open Edit Form.
#[derive(Clone, Debug)]
pub(super) struct FfEdit {
    /// The Freeform feature's index.
    pub feature: usize,
    /// The working definition (written into the feature on every change).
    pub op: FreeformOp,
    /// The picked cage points.
    pub picked: Vec<(usize, usize)>,
    /// Falloff radius in cage faces (0: only the picked points move).
    pub falloff: Real,
    /// A drag in progress: the view plane's point and normal at its start, and every point's
    /// offset then.
    pub drag: Option<(Point3, Vector3, Vec<[Real; 3]>)>,
}

impl CadCraftApp {
    /// Edit Form of Freeform feature `k` (Edit Feature of a Freeform).
    pub(super) fn ff_start(&mut self, k: usize) {
        let Some(Feature::BodyOp(b)) = self.part().features.get(k) else {
            return;
        };
        let BodyOp::Freeform(op) = &b.op else {
            return;
        };
        let op = op.clone();
        self.finish_sketch();
        self.ff_edit = Some(FfEdit {
            feature: k,
            op,
            picked: Vec::new(),
            falloff: 0.0,
            drag: None,
        });
        self.scene_dirty = true;
    }

    /// The Freeform's base plane now.
    fn ff_plane(&self, op: &FreeformOp) -> Option<Plane> {
        crate::part::placement::plane_of(&op.plane, &self.part().body, &self.part().work).ok()
    }

    /// Writes the working definition into the feature (regenerates).
    fn ff_apply(&mut self) {
        let Some(e) = self.ff_edit.as_ref() else {
            return;
        };
        let (k, op) = (e.feature, e.op.clone());
        let Some(Feature::BodyOp(mut b)) = self.part().features.get(k).cloned() else {
            return;
        };
        b.op = BodyOp::Freeform(op);
        match self.part_mut().replace_feature(k, Feature::BodyOp(b)) {
            Ok(_) => {
                self.scene_dirty = true;
                self.file_status = None;
            }
            Err(e) => self.file_status = Some(Err(format!("Edit Form: {e}"))),
        }
    }

    /// The cage point nearest `p` on screen, within the pick radius.
    fn ff_point_at(&self, p: Pos2, rect: Rect) -> Option<(usize, usize)> {
        let e = self.ff_edit.as_ref()?;
        let pl = self.ff_plane(&e.op)?;
        let net = e.op.cage(&pl);
        let mut best: Option<(f32, (usize, usize))> = None;
        for (i, row) in net.iter().enumerate() {
            for (j, q) in row.iter().enumerate() {
                if let Some(s) = self.camera.project(q, rect) {
                    let d = s.distance(p);
                    if d <= PICK_PX && best.is_none_or(|(b, _)| d < b) {
                        best = Some((d, (i, j)));
                    }
                }
            }
        }
        best.map(|(_, ij)| ij)
    }

    /// The offsets after moving the picked points by `delta` (plane frame) from `start`: each
    /// point by its weight (1 for a picked one, a smooth falloff within the radius), mirrored
    /// across the symmetry lines.
    fn ff_moved(e: &FfEdit, start: &[[Real; 3]], delta: [Real; 3]) -> Vec<[Real; 3]> {
        let (rows, nw) = (e.op.rows(), e.op.width_faces);
        let closed = e.op.closed();
        let idx = |i: usize, j: usize| i * (nw + 1) + j;
        let mut out = start.to_vec();
        // The picked points with their mirrors, each with how its move is mirrored.
        let mut seeds: Vec<(usize, usize, [bool; 2])> = Vec::new();
        for &(i, j) in &e.picked {
            for m in e.op.mirrors(i, j) {
                if !seeds.iter().any(|s| (s.0, s.1) == (m.0, m.1)) {
                    seeds.push(m);
                }
            }
        }
        for i in 0..rows {
            for j in 0..=nw {
                // The nearest seed (in cage faces) decides the weight and the mirroring.
                let Some((d, flip)) = seeds
                    .iter()
                    .map(|(si, sj, f)| {
                        // Around a closed cage the short way round.
                        let mut di = (i as Real - *si as Real).abs();
                        if closed {
                            di = di.min(rows as Real - di);
                        }
                        let d = (di.powi(2) + (j as Real - *sj as Real).powi(2)).sqrt();
                        (d, *f)
                    })
                    .min_by(|a, b| a.0.total_cmp(&b.0))
                else {
                    continue;
                };
                let w = if d < 1e-9 {
                    1.0
                } else if e.falloff > 0.0 && d < e.falloff {
                    0.5 * (1.0 + (std::f64::consts::PI * d / e.falloff).cos())
                } else {
                    0.0
                };
                if w == 0.0 {
                    continue;
                }
                let dx = if flip[0] { -delta[0] } else { delta[0] };
                let dy = if flip[1] { -delta[1] } else { delta[1] };
                let k = idx(i, j);
                out[k] = [
                    start[k][0] + w * dx,
                    start[k][1] + w * dy,
                    start[k][2] + w * delta[2],
                ];
            }
        }
        out
    }

    /// Every cage point's offset (row by row).
    fn ff_offsets(op: &FreeformOp) -> Vec<[Real; 3]> {
        (0..op.rows())
            .flat_map(|i| (0..=op.width_faces).map(move |j| (i, j)))
            .map(|(i, j)| op.offset(i, j))
            .collect()
    }

    fn ff_set_offsets(op: &mut FreeformOp, all: &[[Real; 3]]) {
        let nw = op.width_faces;
        for (k, o) in all.iter().enumerate() {
            op.set_offset(k / (nw + 1), k % (nw + 1), *o);
        }
    }

    /// The view's input while Edit Form is open: picks, drags, Esc. Returns whether it took
    /// the pointer (the view does not orbit or select then).
    pub(super) fn ff_view_input(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        rect: Rect,
    ) -> bool {
        if self.ff_edit.is_none() {
            return false;
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.ff_done();
            return true;
        }
        let mods = ui.input(|i| i.modifiers);
        // A drag that starts on a cage point moves the picked ones (picking it first).
        if response.drag_started_by(PointerButton::Primary)
            && let Some(press) = ui.input(|i| i.pointer.press_origin())
            && let Some(ij) = self.ff_point_at(press, rect)
        {
            let Some(e) = self.ff_edit.as_ref() else {
                return false;
            };
            let Some(pl) = self.ff_plane(&e.op) else {
                return false;
            };
            let at = e.op.cage(&pl)[ij.0][ij.1];
            let Some((_, dir)) = self.camera.ray(press, rect) else {
                return false;
            };
            self.checkpoint("Edit Form");
            if let Some(e) = self.ff_edit.as_mut() {
                if !e.picked.contains(&ij) {
                    if !mods.shift {
                        e.picked.clear();
                    }
                    e.picked.push(ij);
                }
                e.drag = Some((at, dir.normalize(), Self::ff_offsets(&e.op)));
            }
            return true;
        }
        if let Some((at, n, start)) = self.ff_edit.as_ref().and_then(|e| e.drag.clone()) {
            if response.dragged_by(PointerButton::Primary)
                && let Some(cur) = response.interact_pointer_pos()
                && let Some((o, d)) = self.camera.ray(cur, rect)
                && let Some(pl) = self.ff_edit.as_ref().and_then(|e| self.ff_plane(&e.op))
            {
                // The cursor's ray met with the view plane through the point.
                let den = d.dot(&n);
                if den.abs() > 1e-12 {
                    let hit = o + d * ((at - o).dot(&n) / den);
                    let w = hit - at;
                    let delta = [
                        w.dot(&pl.x_axis.into_inner()),
                        w.dot(&pl.y_axis().into_inner()),
                        w.dot(&pl.normal.into_inner()),
                    ];
                    if let Some(e) = self.ff_edit.as_mut() {
                        let all = Self::ff_moved(e, &start, delta);
                        Self::ff_set_offsets(&mut e.op, &all);
                    }
                    self.ff_apply();
                }
            }
            if response.drag_stopped() {
                if let Some(e) = self.ff_edit.as_mut() {
                    e.drag = None;
                }
                self.history.break_coalescing();
            }
            return true;
        }
        if response.clicked_by(PointerButton::Primary) {
            let hit = response
                .interact_pointer_pos()
                .and_then(|p| self.ff_point_at(p, rect));
            if let Some(e) = self.ff_edit.as_mut() {
                match hit {
                    Some(ij) if mods.ctrl => e.picked.retain(|x| *x != ij),
                    Some(ij) if mods.shift => {
                        if !e.picked.contains(&ij) {
                            e.picked.push(ij);
                        }
                    }
                    Some(ij) => e.picked = vec![ij],
                    None if !mods.shift && !mods.ctrl => e.picked.clear(),
                    None => {}
                }
            }
            return true;
        }
        false
    }

    /// The cage over the view: its lines, its points (picked ones larger, in the highlight
    /// colour), the point under the cursor ringed.
    pub(super) fn ff_draw(&self, painter: &egui::Painter, rect: Rect, hover: Option<Pos2>) {
        let Some(e) = self.ff_edit.as_ref() else {
            return;
        };
        let Some(pl) = self.ff_plane(&e.op) else {
            return;
        };
        let net = e.op.cage(&pl);
        let line = Stroke::new(1.2, Color32::from_rgb(70, 110, 200));
        let pr = |q: &Point3| self.camera.project(q, rect);
        for (i, row) in net.iter().enumerate() {
            for (j, q) in row.iter().enumerate() {
                let Some(a) = pr(q) else { continue };
                if let Some(b) = row.get(j + 1).and_then(pr) {
                    painter.line_segment([a, b], line);
                }
                // A closed cage's last row joins its first.
                let next = if e.op.closed() {
                    (i + 1) % net.len()
                } else {
                    i + 1
                };
                if let Some(b) = net.get(next).and_then(|r| r.get(j)).and_then(pr) {
                    painter.line_segment([a, b], line);
                }
            }
        }
        let hot = hover.and_then(|p| self.ff_point_at(p, rect));
        for (i, row) in net.iter().enumerate() {
            for (j, q) in row.iter().enumerate() {
                let Some(a) = pr(q) else { continue };
                let picked = e.picked.contains(&(i, j));
                let (r, c) = if picked {
                    (4.5, Color32::from_rgb(40, 200, 90))
                } else {
                    (3.0, Color32::from_rgb(40, 80, 190))
                };
                painter.rect_filled(
                    Rect::from_center_size(a, egui::vec2(2.0 * r, 2.0 * r)),
                    1.0,
                    c,
                );
                if hot == Some((i, j)) {
                    painter.circle_stroke(a, 7.0, Stroke::new(1.5, Color32::from_rgb(40, 200, 90)));
                }
            }
        }
    }

    /// Ends Edit Form (its changes are already in the feature).
    pub(super) fn ff_done(&mut self) {
        self.ff_edit = None;
        self.history.break_coalescing();
        self.scene_dirty = true;
    }

    /// The status bar's prompt.
    pub(super) fn ff_prompt(&self) -> Option<String> {
        let e = self.ff_edit.as_ref()?;
        Some(if e.picked.is_empty() {
            "Edit Form: click a cage point (Shift adds, Ctrl removes), drag it to shape the surface; Esc or Done ends".into()
        } else {
            format!(
                "Edit Form: {} point(s) picked: drag one to move them in the view plane, or type the offset",
                e.picked.len()
            )
        })
    }

    /// Edit Form's panel: the picked points' offset, the falloff, Reset and Done.
    pub(super) fn ff_panel(&mut self, ui: &mut egui::Ui) {
        let Some(mut e) = self.ff_edit.clone() else {
            return;
        };
        self.panel_header(ui, Icon::FfPlane, "Edit Form");
        let mut changed = false;
        psection(ui, "ff_points", "Points", |ui| {
            plabel(ui, "Picked");
            ui.label(format!(
                "{} of {} × {}",
                e.picked.len(),
                e.op.rows(),
                e.op.width_faces + 1
            ));
            ui.end_row();
            if let Some(&(i, j)) = e.picked.first() {
                let mut o = e.op.offset(i, j);
                let before = o;
                for (k, axis) in ["X", "Y", "Z"].iter().enumerate() {
                    plabel(ui, &format!("Offset {axis}"));
                    ui.add(ExprValue::new(&mut o[k]).suffix(" mm"))
                        .on_hover_text("The picked points' offset from rest, in the base plane's frame (Z along its normal)");
                    ui.end_row();
                }
                if o != before {
                    // Every picked point (and its mirrors) by the same change.
                    let delta = [o[0] - before[0], o[1] - before[1], o[2] - before[2]];
                    let start = Self::ff_offsets(&e.op);
                    let saved = e.falloff;
                    e.falloff = 0.0;
                    let all = Self::ff_moved(&e, &start, delta);
                    e.falloff = saved;
                    Self::ff_set_offsets(&mut e.op, &all);
                    changed = true;
                }
            }
            plabel(ui, "Falloff");
            ui.add(ExprValue::new(&mut e.falloff).range(0.0..=64.0))
                .on_hover_text("Soft Modification: points within this many cage faces of a picked one move with it, less the farther they are (smooth); 0 moves only the picked points");
            ui.end_row();
            plabel(ui, "Symmetry");
            ui.label(match e.op.symmetry {
                [true, true] => "X and Y",
                [true, false] => "X",
                [false, true] => "Y",
                _ => "none",
            });
            ui.end_row();
        });
        ui.horizontal(|ui| {
            if ui
                .button("Reset")
                .on_hover_text("Every point back at rest")
                .clicked()
            {
                e.op.moved.clear();
                changed = true;
            }
            if ui
                .button("Sizes and Faces...")
                .on_hover_text(
                    "The Freeform Plane panel: sizes, faces, symmetry (moved points are kept)",
                )
                .clicked()
            {
                let k = e.feature;
                let op = e.op.clone();
                self.ff_done();
                use super::part_planned::PartPlanned;
                let tube = op.closed();
                self.start_part_planned(if tube {
                    PartPlanned::FreeformCylinder
                } else {
                    PartPlanned::FreeformPlane
                });
                if let Some(d) = self.part_planned.as_mut() {
                    d.feature = Some(k);
                    let [a, b, fa, fb] = if tube {
                        ["Diameter", "Height", "Around Faces", "Height Faces"]
                    } else {
                        ["Length", "Width", "Length Faces", "Width Faces"]
                    };
                    d.set_num(0, a, op.length);
                    d.set_num(0, b, op.width);
                    d.set_num(0, fa, op.length_faces as Real);
                    d.set_num(0, fb, op.width_faces as Real);
                    d.set_check(0, "Symmetry X", op.symmetry[0]);
                    d.set_check(0, "Symmetry Y", op.symmetry[1]);
                }
                return;
            }
            if ui.button("Done").clicked() {
                e.drag = None;
                self.ff_edit = Some(e.clone());
                self.ff_done();
            }
        });
        ui.label(
            RichText::new(
                "Drag a cage point in the view to move the picked points in the view plane.",
            )
            .weak(),
        );
        if self.ff_edit.is_none() {
            return;
        }
        if changed {
            self.checkpoint("Edit Form");
            self.ff_edit = Some(e);
            self.ff_apply();
            self.history.break_coalescing();
        } else {
            self.ff_edit = Some(e);
        }
    }
}
