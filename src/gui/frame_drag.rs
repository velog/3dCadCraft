//! Lengthen/Shorten in the view (spec 6.1 §3.14, owner's remark
//! 2026-11-20: every command previews and its values can be dragged): each
//! picked member's changed end is drawn where the panel's offsets put it —
//! the run's extension and the section's outline there, green when it grows,
//! red when it shrinks — with an arrow along the member that drags the
//! offset of that end (the panel's field; Shift: 0.1 mm steps). Nothing is
//! built before OK.

use super::CadCraftApp;
use super::asm_planned::AsmPlanned;
use super::frame_select::pick_name;
use super::part_planned::{PartDialog, Planned};
use crate::assembly::frame;
use crate::kernel::{Point3, Real, Vector3};
use eframe::egui::{self, Color32, Pos2, Rect, Stroke};

/// The arrow's length on the screen, px.
const ARROW_PX: f32 = 42.0;
/// How near a press must be to the arrow, px.
const GRIP_PX: f32 = 8.0;

/// One changed end: where it is now, the way out along the member, the
/// offset the panel gives it, the field that holds it and the section's
/// outline at the new end (active assembly coordinates).
struct End {
    at: Point3,
    out: Vector3,
    offset: Real,
    field: &'static str,
    outline: Vec<Point3>,
}

/// A held Lengthen/Shorten arrow: the field, its value at the press, the
/// press, the arrow's screen direction and px per mm along it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct EndHeld {
    field: &'static str,
    start: Real,
    press: Pos2,
    dir: egui::Vec2,
    px_per_mm: f32,
}

fn is_lengthen(d: &PartDialog) -> bool {
    d.kind == Planned::Asm(AsmPlanned::LengthenShorten)
}

impl CadCraftApp {
    /// The ends the open Lengthen/Shorten panel changes.
    fn lengthen_ends(&self, d: &PartDialog) -> Vec<End> {
        let Some(a) = self.frame_asm() else {
            return Vec::new();
        };
        let linked = self.frame_linked();
        let place = self.frame_place();
        let kind = d.index(0, "Offset Type");
        let o1 = d.num(0, "Offset").unwrap_or(0.0);
        let o2 = d.num(0, "Offset 2").unwrap_or(0.0);
        let mut out = Vec::new();
        for w in d.picks_of("members") {
            let Some(o) = a.occurrences.iter().find(|o| o.name == pick_name(w)) else {
                continue;
            };
            let Some(m) = a
                .frames
                .iter()
                .flat_map(|f| &f.members)
                .find(|m| m.occurrence == o.id)
            else {
                continue;
            };
            let Some((s, e)) = frame::member_run(&self.project, &linked, a, o.id) else {
                continue;
            };
            let Ok((p, q)) = frame::skeleton_line(&self.project, &linked, a, &m.skeleton) else {
                continue;
            };
            let click = d
                .clicks
                .iter()
                .rev()
                .find(|(l, _)| l == w)
                .map(|(_, c)| place.inverse() * Point3::new(c[0], c[1], c[2]));
            // As OK: the end nearer the click, else the run's end.
            let near = match click {
                Some(c) => usize::from((c - e).norm() < (c - s).norm()),
                None => 1,
            };
            let far = 1 - near;
            let ends: Vec<(usize, Real, &'static str)> = match kind {
                1 => vec![(far, o1, "Offset")],
                2 => vec![(near, o1, "Offset"), (far, o1, "Offset")],
                3 => vec![(near, o1, "Offset"), (far, o2, "Offset 2")],
                _ => vec![(near, o1, "Offset")],
            };
            let len = (e - s).norm();
            let t = m
                .section()
                .and_then(|sec| frame::placement(&sec, p, q, &m.orientation).map(|t| (sec, t)));
            for (k, off, field) in ends {
                let (at, other) = if k == 1 { (e, s) } else { (s, e) };
                let Some(dir) = (at - other).try_normalize(1e-9) else {
                    continue;
                };
                // The outline at the new end (z from the run's start).
                let z = if k == 1 { len + off } else { -off };
                let outline = t
                    .as_ref()
                    .map(|(sec, t)| {
                        sec.outline()
                            .iter()
                            .map(|p| place * t.transform_point(&Point3::new(p.x, p.y, z)))
                            .collect()
                    })
                    .unwrap_or_default();
                out.push(End {
                    at: place * at,
                    out: place.transform_vector(&dir),
                    offset: off,
                    field,
                    outline,
                });
            }
        }
        out
    }

    /// An end's arrow on the screen (from, to) and the px per mm along it.
    fn end_arrow(&self, e: &End, rect: Rect) -> Option<(Pos2, Pos2, f32)> {
        let tip = e.at + e.out * e.offset;
        let a = self.camera.project(&tip, rect)?;
        let b = self.camera.project(&(tip + e.out), rect)?;
        let px = a.distance(b);
        if px < 1e-4 {
            return None;
        }
        let dir = (b - a) / px;
        Some((a, a + dir * ARROW_PX, px))
    }

    /// The preview and the arrows of Lengthen/Shorten (the arrow under the
    /// cursor or held lit, its value beside the cursor).
    pub(super) fn lengthen_preview(
        &self,
        d: &PartDialog,
        painter: &egui::Painter,
        rect: Rect,
        pointer: Option<Pos2>,
    ) {
        if !is_lengthen(d) {
            return;
        }
        let scheme = super::theme::canvas();
        let held = self.asm.frame_end_grip.map(|h| h.field);
        for e in self.lengthen_ends(d) {
            let col = if e.offset >= 0.0 {
                Color32::from_rgb(60, 200, 90)
            } else {
                Color32::from_rgb(230, 70, 60)
            };
            let tip = e.at + e.out * e.offset;
            if let (Some(a), Some(b)) = (
                self.camera.project(&e.at, rect),
                self.camera.project(&tip, rect),
            ) {
                painter.line_segment([a, b], Stroke::new(4.0, col));
                painter.circle(a, 4.0, col, Stroke::new(1.0, Color32::WHITE));
            }
            let ring: Vec<Pos2> = e
                .outline
                .iter()
                .filter_map(|p| self.camera.project(p, rect))
                .collect();
            if ring.len() > 2 {
                painter.add(egui::Shape::closed_line(ring, Stroke::new(1.5, col)));
            }
            let Some((a, b, _)) = self.end_arrow(&e, rect) else {
                continue;
            };
            let hot = held == Some(e.field)
                || pointer.is_some_and(|p| seg_dist(p, a, b) <= GRIP_PX);
            let c = if hot {
                scheme.handle_hot
            } else {
                scheme.handle
            };
            painter.line_segment([a, b], Stroke::new(2.0, c));
            let dir = (b - a).normalized();
            let n = egui::vec2(-dir.y, dir.x);
            painter.add(egui::Shape::convex_polygon(
                vec![b + dir * 9.0, b + n * 5.0, b - n * 5.0],
                c,
                Stroke::NONE,
            ));
            if hot && let Some(p) = pointer {
                painter.text(
                    p + egui::vec2(14.0, -14.0),
                    egui::Align2::LEFT_BOTTOM,
                    format!("{}: {:.1} mm", e.field, e.offset),
                    egui::FontId::proportional(13.0),
                    scheme.handle_hot,
                );
            }
        }
    }

    /// A drag of a Lengthen/Shorten arrow edits its field (`true`: taken).
    pub(super) fn lengthen_grip(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        rect: Rect,
    ) -> bool {
        let Some(d) = self.part_planned.as_ref().filter(|d| is_lengthen(d)) else {
            self.asm.frame_end_grip = None;
            return false;
        };
        if response.drag_started_by(egui::PointerButton::Primary)
            && let Some(press) = ui.input(|i| i.pointer.press_origin())
        {
            let hit = self.lengthen_ends(d).into_iter().find_map(|e| {
                let (a, b, px) = self.end_arrow(&e, rect)?;
                (seg_dist(press, a, b) <= GRIP_PX).then(|| EndHeld {
                    field: e.field,
                    start: e.offset,
                    press,
                    dir: (b - a).normalized(),
                    px_per_mm: px,
                })
            });
            self.asm.frame_end_grip = hit;
            return hit.is_some();
        }
        let Some(h) = self.asm.frame_end_grip else {
            return false;
        };
        if !response.dragged() {
            self.asm.frame_end_grip = None;
            return false;
        }
        if let Some(now) = response.interact_pointer_pos() {
            let step = if ui.input(|i| i.modifiers.shift) {
                0.1
            } else {
                1.0
            };
            let along = ((now - h.press).dot(h.dir) / h.px_per_mm) as Real;
            let v = ((h.start + along) / step).round() * step;
            if let Some(d) = self.part_planned.as_mut() {
                d.set_num(0, h.field, v);
            }
        }
        true
    }
}

fn seg_dist(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}
