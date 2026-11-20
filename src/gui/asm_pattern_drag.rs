//! Pattern Component's handles in the view (spec 3.0 §3.7, rule 12; owner's
//! remark 2026-11-20): on the Rectangular tab an arrow at the second column
//! (and the second row) along its direction drags Column / Row Spacing; on
//! the Circular tab an arrow at the second element, tangent to its circle
//! about the axis, drags the Angle. The arrows edit the panel's fields (the
//! preview follows); Shift takes fine steps.

use super::CadCraftApp;
use super::asm_planned::AsmPlanned;
use super::part_planned::{PartDialog, Planned};
use crate::assembly::pattern;
use crate::assembly::relation::GeomRef;
use crate::kernel::{Point3, Real, Vector3};
use eframe::egui::{self, Pos2, Rect, Stroke};

/// The arrow's length on the screen, px.
const ARROW_PX: f32 = 42.0;
/// How near a press must be to an arrow, px.
const GRIP_PX: f32 = 8.0;
/// Tabs of the Pattern panel with handles.
const RECTANGULAR: usize = 1;
const CIRCULAR: usize = 2;

/// One handle: the tab and field it edits, where it stands, its direction,
/// the field's value and the field's units per mm along the arrow.
struct Arrow {
    tab: usize,
    field: &'static str,
    at: Point3,
    dir: Vector3,
    value: Real,
    per_mm: Real,
}

/// A held pattern arrow.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PatternHeld {
    tab: usize,
    field: &'static str,
    start: Real,
    press: Pos2,
    dir: egui::Vec2,
    px_per_mm: f32,
    per_mm: Real,
}

fn is_pattern(d: &PartDialog) -> bool {
    d.kind == Planned::Asm(AsmPlanned::Pattern)
}

impl CadCraftApp {
    /// The middle of the sources' boxes (active assembly coordinates).
    fn pattern_base(&self, d: &PartDialog) -> Option<Point3> {
        let names = d.picks_of("components");
        let ids: Vec<(u32, bool)> = names
            .iter()
            .filter_map(|w| {
                let name = w.split(" · ").next().unwrap_or(w);
                if w.ends_with(super::asm_pattern::MEMBER_TAG) {
                    let a = self.frame_asm()?;
                    a.occurrences
                        .iter()
                        .find(|o| o.name == name)
                        .map(|o| (o.id, true))
                } else {
                    let a = self.assembly()?;
                    a.occurrences
                        .iter()
                        .find(|o| o.name == name)
                        .map(|o| (o.id, false))
                }
            })
            .collect();
        let (lo, hi) = ids
            .iter()
            .filter_map(|&(id, member)| {
                if member {
                    self.frame_member_box(id)
                } else {
                    self.asm_box(id)
                }
            })
            .reduce(|(a, b), (c, e)| (a.inf(&c), b.sup(&e)))?;
        Some(nalgebra::center(&lo, &hi))
    }

    /// A reference's geometry in active assembly coordinates.
    fn pattern_geom(&self, r: &GeomRef) -> Option<crate::assembly::Geom> {
        let a = self.assembly()?;
        let g = self.asm_local_in(a, r).ok()?;
        Some(match a.find(r.occurrence()) {
            Some(o) => g.moved(&o.transform),
            None => g,
        })
    }

    /// The open Pattern panel's handles.
    fn pattern_arrows(&self, d: &PartDialog) -> Vec<Arrow> {
        let mut out = Vec::new();
        let Some(base) = self.pattern_base(d) else {
            return out;
        };
        let r = |key: &str| d.refs.get(key).and_then(|v| v.first()).cloned();
        match d.tab() {
            RECTANGULAR => {
                for (key, flip, field) in [
                    ("column", "Flip column", "Column Spacing"),
                    ("row", "Flip row", "Row Spacing"),
                ] {
                    let Some(dir) = r(key)
                        .and_then(|g| self.pattern_geom(&g))
                        .and_then(|g| pattern::direction(&g).ok())
                    else {
                        continue;
                    };
                    let dir = if d.check(RECTANGULAR, flip) { -dir } else { dir };
                    let value = d.num(RECTANGULAR, field).unwrap_or(0.0);
                    out.push(Arrow {
                        tab: RECTANGULAR,
                        field,
                        at: base + dir * value,
                        dir,
                        value,
                        per_mm: 1.0,
                    });
                }
            }
            CIRCULAR => {
                let Some((p, n)) = r("axis")
                    .and_then(|g| self.pattern_geom(&g))
                    .and_then(|g| pattern::axis_of(&g).ok())
                else {
                    return out;
                };
                let n = if d.check(CIRCULAR, "Flip") { -n } else { n };
                let radial = (base - p) - n * (base - p).dot(&n);
                let radius = radial.norm();
                if radius < 1e-6 {
                    return out;
                }
                let value = d.num(CIRCULAR, "Angle").unwrap_or(0.0);
                let rot = nalgebra::Rotation3::from_axis_angle(
                    &nalgebra::Unit::new_normalize(n),
                    value.to_radians(),
                );
                let at_r = rot * radial;
                let Some(dir) = n.cross(&at_r).try_normalize(1e-12) else {
                    return out;
                };
                out.push(Arrow {
                    tab: CIRCULAR,
                    field: "Angle",
                    at: base + (at_r - radial),
                    dir,
                    value,
                    per_mm: (1.0 / radius).to_degrees(),
                });
            }
            _ => {}
        }
        out
    }

    /// An arrow on the screen (from, to) and px per mm along it.
    fn pattern_arrow_screen(&self, a: &Arrow, rect: Rect) -> Option<(Pos2, Pos2, f32)> {
        let s = self.camera.project(&a.at, rect)?;
        let t = self.camera.project(&(a.at + a.dir), rect)?;
        let px = s.distance(t);
        if px < 1e-4 {
            return None;
        }
        Some((s, s + (t - s) / px * ARROW_PX, px))
    }

    /// Draws the open Pattern panel's handles (the held or hovered one lit,
    /// its value beside the cursor).
    pub(super) fn pattern_handles_draw(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        pointer: Option<Pos2>,
    ) {
        let Some(d) = self.part_planned.as_ref().filter(|d| is_pattern(d)) else {
            return;
        };
        let scheme = super::theme::canvas();
        let held = self.asm.pattern_grip.map(|h| h.field);
        for a in self.pattern_arrows(d) {
            let Some((s, t, _)) = self.pattern_arrow_screen(&a, rect) else {
                continue;
            };
            let hot =
                held == Some(a.field) || pointer.is_some_and(|p| seg_dist(p, s, t) <= GRIP_PX);
            let c = if hot {
                scheme.handle_hot
            } else {
                scheme.handle
            };
            painter.circle_filled(s, 3.5, c);
            painter.line_segment([s, t], Stroke::new(2.0, c));
            let dir = (t - s).normalized();
            let nn = egui::vec2(-dir.y, dir.x);
            painter.add(egui::Shape::convex_polygon(
                vec![t + dir * 9.0, t + nn * 5.0, t - nn * 5.0],
                c,
                Stroke::NONE,
            ));
            if hot && let Some(p) = pointer {
                let unit = if a.tab == CIRCULAR { "°" } else { " mm" };
                painter.text(
                    p + egui::vec2(14.0, -14.0),
                    egui::Align2::LEFT_BOTTOM,
                    format!("{}: {:.1}{unit}", a.field, a.value),
                    egui::FontId::proportional(13.0),
                    scheme.handle_hot,
                );
            }
        }
    }

    /// A drag of a Pattern handle edits its field (`true`: taken).
    pub(super) fn pattern_grip(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        rect: Rect,
    ) -> bool {
        let Some(d) = self.part_planned.as_ref().filter(|d| is_pattern(d)) else {
            self.asm.pattern_grip = None;
            return false;
        };
        if response.drag_started_by(egui::PointerButton::Primary)
            && let Some(press) = ui.input(|i| i.pointer.press_origin())
        {
            let hit = self.pattern_arrows(d).into_iter().find_map(|a| {
                let (s, t, px) = self.pattern_arrow_screen(&a, rect)?;
                (seg_dist(press, s, t) <= GRIP_PX).then(|| PatternHeld {
                    tab: a.tab,
                    field: a.field,
                    start: a.value,
                    press,
                    dir: (t - s).normalized(),
                    px_per_mm: px,
                    per_mm: a.per_mm,
                })
            });
            self.asm.pattern_grip = hit;
            return hit.is_some();
        }
        let Some(h) = self.asm.pattern_grip else {
            return false;
        };
        if !response.dragged() {
            self.asm.pattern_grip = None;
            return false;
        }
        if let Some(now) = response.interact_pointer_pos() {
            let fine = ui.input(|i| i.modifiers.shift);
            let step = match (h.tab == CIRCULAR, fine) {
                (true, false) => 5.0,
                (true, true) => 1.0,
                (false, false) => 1.0,
                (false, true) => 0.1,
            };
            let along = ((now - h.press).dot(h.dir) / h.px_per_mm) as Real * h.per_mm;
            let v = ((h.start + along) / step).round() * step;
            if let Some(d) = self.part_planned.as_mut() {
                d.set_num(h.tab, h.field, v);
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
