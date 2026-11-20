//! Insert Frame's and Change's manipulators in the view (spec 6.1 §3.1,
//! rule 12): on the last picked line or member an arrow along the section's
//! horizontal axis (Horizontal Offset), one along its vertical axis (Vertical
//! Offset) and a ring about the line (Rotation Angle). A drag edits the
//! panel's field, the same value the panel shows; the members follow at OK.

use eframe::egui::{self, Pos2, Rect, Stroke};

use super::CadCraftApp;
use super::asm_planned::AsmPlanned;
use super::part_planned::{PartDialog, Planned};
use crate::assembly::frame;
use crate::kernel::{Point3, Real, Vector3};
use crate::part::frame::Library;

/// The arrows' length and the ring's radius on the screen, px.
const ARROW_PX: f32 = 46.0;
/// How near the pointer must press to take a grip, px.
const GRIP_PX: f32 = 8.0;
/// How near a click must be to an anchor point, px.
const ANCHOR_PX: f32 = 7.0;

/// The arrows on the screen (grip, from, to), the anchor's place there and
/// the mm per pixel.
type Screen = ([(Grip, Pos2, Pos2); 2], Pos2, Real);

/// What a grip edits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Grip {
    /// Horizontal Offset.
    Horizontal,
    /// Vertical Offset.
    Vertical,
    /// Rotation Angle.
    Rotation,
}

impl Grip {
    fn field(self) -> &'static str {
        match self {
            Grip::Horizontal => "Horizontal Offset",
            Grip::Vertical => "Vertical Offset",
            Grip::Rotation => "Rotation Angle",
        }
    }
}

/// A held grip: what it edits, the field's value at the press, where the
/// press was, the ring angle at the press (rotation).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Held {
    grip: Grip,
    start: Real,
    press: Pos2,
    angle0: Real,
}

/// The manipulators of the open panel in assembly coordinates: the line's
/// middle, its direction, the section's horizontal / vertical axes and the
/// anchor's place (the middle moved by the offsets).
struct Frame3 {
    mid: Point3,
    dir: Vector3,
    u: Vector3,
    v: Vector3,
    base: Point3,
    /// The horizontal axis at Rotation Angle 0 (the ring's fixed reference).
    u0: Vector3,
    /// The section's anchor points at the line's middle, in the order of
    /// `Anchor::ALL` (the panel's Anchor list).
    anchors: Vec<Point3>,
    /// The section's outer outline at the line's middle.
    outline: Vec<Point3>,
}

/// Smallest size of the anchor points' group on the screen, px: a section
/// smaller than that on screen is drawn magnified about its centre, so its
/// points can be told apart and clicked.
const ANCHOR_SPAN_PX: f32 = 70.0;

fn takes(d: &PartDialog) -> bool {
    matches!(
        d.kind,
        Planned::Asm(AsmPlanned::InsertFrame | AsmPlanned::FrameChange)
    ) && d.members.list.is_empty()
}

impl CadCraftApp {
    /// The panel's subject line in assembly coordinates (the last picked
    /// line, else the last picked member's run).
    fn manip_line(&self, d: &PartDialog) -> Option<(Point3, Point3)> {
        let linked = self.frame_linked();
        if let Some(s) = d.skeletons.last() {
            let a = self.assembly()?;
            return frame::skeleton_line(&self.project, &linked, a, s).ok();
        }
        let occ = *self.picked_members(d, "members").first()?;
        // The member's own run, without its orientation's Flip.
        let a = self.frame_asm()?;
        let m = a
            .frames
            .iter()
            .flat_map(|f| &f.members)
            .find(|m| m.occurrence == occ)?;
        let (p, q) = frame::skeleton_line(&self.project, &linked, a, &m.skeleton).ok()?;
        let place = self.frame_place();
        Some((place * p, place * q))
    }

    fn manip_frame(&self, d: &PartDialog) -> Option<Frame3> {
        if !takes(d) {
            return None;
        }
        let (p, q) = self.manip_line(d)?;
        let o = super::frame::insert_options(d)?;
        let lib = Library::current();
        let s = lib.find(&o.family, &o.size)?;
        let mut or = o.orientation;
        or.mirror = false;
        let t = frame::placement(s, p, q, &or)?;
        let u = t.transform_vector(&Vector3::x());
        let v = t.transform_vector(&Vector3::y());
        let dir = (q - p).try_normalize(1e-9)?;
        let mid = nalgebra::center(&p, &q);
        let base = mid + u * or.h_offset + v * or.v_offset;
        let flat = frame::Orientation { angle: 0.0, ..or };
        let u0 = frame::placement(s, p, q, &flat)?.transform_vector(&Vector3::x());
        let half = (q - p).norm() / 2.0;
        let anchors = frame::Anchor::ALL
            .iter()
            .map(|a| {
                let (x, y) = a.point(s);
                t.transform_point(&Point3::new(x, y, half))
            })
            .collect();
        let outline = s
            .outline()
            .iter()
            .map(|p| t.transform_point(&Point3::new(p.x, p.y, half)))
            .collect();
        Some(Frame3 {
            mid,
            dir,
            u,
            v,
            base,
            u0,
            anchors,
            outline,
        })
    }

    /// The anchor points and the outline on the screen, magnified about the
    /// section's centre to [`ANCHOR_SPAN_PX`] when smaller (true then).
    fn anchor_screen(&self, f: &Frame3, rect: Rect) -> Option<(Vec<Pos2>, Vec<Pos2>, bool)> {
        let pts: Vec<Pos2> = f
            .anchors
            .iter()
            .map(|w| self.camera.project(w, rect))
            .collect::<Option<_>>()?;
        let outline: Vec<Pos2> = f
            .outline
            .iter()
            .filter_map(|w| self.camera.project(w, rect))
            .collect();
        let c = *pts.first()?;
        let span = pts.iter().map(|p| p.distance(c)).fold(0.0f32, f32::max) * 2.0;
        if span >= ANCHOR_SPAN_PX {
            return Some((pts, outline, false));
        }
        let k = ANCHOR_SPAN_PX / span.max(1.0);
        let grow = |p: &Pos2| c + (*p - c) * k;
        Some((
            pts.iter().map(grow).collect(),
            outline.iter().map(grow).collect(),
            true,
        ))
    }

    /// Screen ends of the arrows and the ring (centre, radius) with the mm
    /// per pixel there.
    fn manip_screen(&self, f: &Frame3, rect: Rect) -> Option<Screen> {
        let b = self.camera.project(&f.base, rect)?;
        let probe = self.camera.project(&(f.base + f.u * 1.0), rect)?;
        let probe_v = self.camera.project(&(f.base + f.v * 1.0), rect)?;
        let px_per_mm = (b.distance(probe) as Real).max(b.distance(probe_v) as Real);
        if px_per_mm < 1e-9 {
            return None;
        }
        let end = |w: Vector3| -> Option<Pos2> {
            let s = self.camera.project(&(f.base + w), rect)?;
            let n = (s - b).normalized();
            (n.length() > 0.5).then_some(b + n * ARROW_PX)
        };
        let h = end(f.u)?;
        let v = end(f.v)?;
        Some((
            [(Grip::Horizontal, b, h), (Grip::Vertical, b, v)],
            b,
            1.0 / px_per_mm,
        ))
    }

    /// The ring's grip: on the circle about the line through the anchor's
    /// middle, at the section's horizontal axis.
    fn ring_points(&self, f: &Frame3, rect: Rect, mm_per_px: Real) -> Vec<Pos2> {
        let r = ARROW_PX as Real * 1.3 * mm_per_px;
        let w = f.dir.cross(&f.u);
        (0..=48)
            .filter_map(|k| {
                let a = k as Real / 48.0 * std::f64::consts::TAU;
                self.camera
                    .project(&(f.mid + (f.u * a.cos() + w * a.sin()) * r), rect)
            })
            .collect()
    }

    /// The angle about the line of the eye ray through `pos` (from the
    /// section's horizontal axis), `None` looking along the ring's plane.
    fn ring_angle(&self, f: &Frame3, rect: Rect, pos: Pos2) -> Option<Real> {
        let (o, d) = self.camera.ray(pos, rect)?;
        let den = d.dot(&f.dir);
        if den.abs() < 1e-6 {
            return None;
        }
        let hit = o + d * ((f.mid - o).dot(&f.dir) / den);
        let r = hit - f.mid;
        let w = f.dir.cross(&f.u0);
        Some(r.dot(&w).atan2(r.dot(&f.u0)))
    }

    /// Draws the manipulators of Insert Frame / Change (the held or hovered
    /// grip lit).
    pub(super) fn frame_manip_draw(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        pointer: Option<Pos2>,
    ) {
        let Some(d) = self.part_planned.as_ref() else {
            return;
        };
        let Some(f) = self.manip_frame(d) else {
            return;
        };
        let Some((arrows, _, mm)) = self.manip_screen(&f, rect) else {
            return;
        };
        let scheme = super::theme::canvas();
        let held = self.asm.frame_grip.map(|h| h.grip);
        let hot = held.or_else(|| pointer.and_then(|p| self.manip_hit(&f, rect, p)));
        let col = |g: Grip| {
            if hot == Some(g) {
                scheme.handle_hot
            } else {
                scheme.handle
            }
        };
        for (g, a, b) in arrows {
            painter.line_segment([a, b], Stroke::new(2.0, col(g)));
            let dir = (b - a).normalized();
            let n = egui::vec2(-dir.y, dir.x);
            painter.add(egui::Shape::convex_polygon(
                vec![b + dir * 9.0, b + n * 5.0, b - n * 5.0],
                col(g),
                Stroke::NONE,
            ));
        }
        // The anchor points (Inventor's buttons on the section): a click
        // takes that anchor, the current one is filled. A section small on
        // the screen is drawn magnified with its outline (spec 6.1 §3.14).
        let current = d.index(0, "Anchor");
        if let Some((pts, outline, grown)) = self.anchor_screen(&f, rect) {
            if grown && outline.len() > 2 {
                painter.add(egui::Shape::closed_line(
                    outline,
                    Stroke::new(1.5, scheme.handle.gamma_multiply(0.8)),
                ));
            }
            let mut hot_name = None;
            for (k, q) in pts.iter().enumerate() {
                let near = pointer.is_some_and(|p| p.distance(*q) < ANCHOR_PX);
                let c = if near {
                    hot_name = super::frame_list::ANCHOR_NAMES.get(k).copied();
                    scheme.handle_hot
                } else {
                    scheme.handle
                };
                if k == current {
                    painter.circle_filled(*q, 4.5, c);
                } else {
                    painter.circle_stroke(*q, 4.0, Stroke::new(1.5, c));
                }
            }
            if let (Some(n), Some(p)) = (hot_name, pointer) {
                painter.text(
                    p + egui::vec2(12.0, 12.0),
                    egui::Align2::LEFT_TOP,
                    format!("Anchor: {n}"),
                    egui::FontId::proportional(12.0),
                    scheme.handle_hot,
                );
            }
        }
        let ring = self.ring_points(&f, rect, mm);
        painter.add(egui::Shape::line(
            ring.clone(),
            Stroke::new(1.5, col(Grip::Rotation)),
        ));
        if let Some(g) = ring.first() {
            painter.circle_filled(*g, 5.0, col(Grip::Rotation));
        }
        if let (Some(g), Some(p)) = (hot, pointer) {
            let d = self.part_planned.as_ref();
            let v = d.and_then(|d| d.num(0, g.field())).unwrap_or(0.0);
            let unit = if g == Grip::Rotation { "°" } else { " mm" };
            painter.text(
                p + egui::vec2(14.0, -14.0),
                egui::Align2::LEFT_BOTTOM,
                format!("{}: {v:.1}{unit}", g.field()),
                egui::FontId::proportional(13.0),
                scheme.handle_hot,
            );
        }
    }

    /// A click on one of the section's anchor points: the panel's Anchor
    /// takes it (`false`: no point there).
    pub(super) fn frame_anchor_dot_click(&mut self, p: Pos2, rect: Rect) -> bool {
        let Some(f) = self.part_planned.as_ref().and_then(|d| self.manip_frame(d)) else {
            return false;
        };
        let Some((pts, _, _)) = self.anchor_screen(&f, rect) else {
            return false;
        };
        let hit = pts
            .iter()
            .enumerate()
            .map(|(k, q)| (k, q.distance(p)))
            .filter(|(_, d)| *d < ANCHOR_PX)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        let Some((k, _)) = hit else {
            return false;
        };
        if let Some(d) = self.part_planned.as_mut() {
            d.set_index(0, "Anchor", k);
        }
        true
    }

    /// The grip under `p`.
    fn manip_hit(&self, f: &Frame3, rect: Rect, p: Pos2) -> Option<Grip> {
        let (arrows, _, mm) = self.manip_screen(f, rect)?;
        for (g, a, b) in arrows {
            if seg_dist(p, a, b) <= GRIP_PX {
                return Some(g);
            }
        }
        let ring = self.ring_points(f, rect, mm);
        ring.windows(2)
            .any(|w| seg_dist(p, w[0], w[1]) <= GRIP_PX)
            .then_some(Grip::Rotation)
    }

    /// A drag on a manipulator (`true` while one is held: the view does not
    /// orbit or box-select). The panel's field follows the pointer: offsets
    /// to 1 mm (0.1 with Shift), the angle to 5° (1° with Shift).
    pub(super) fn frame_grip(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        rect: Rect,
    ) -> bool {
        let Some(f) = self.part_planned.as_ref().and_then(|d| self.manip_frame(d)) else {
            self.asm.frame_grip = None;
            return false;
        };
        if response.drag_started_by(egui::PointerButton::Primary)
            && let Some(press) = ui.input(|i| i.pointer.press_origin())
        {
            let Some(grip) = self.manip_hit(&f, rect, press) else {
                return false;
            };
            let start = self
                .part_planned
                .as_ref()
                .and_then(|d| d.num(0, grip.field()))
                .unwrap_or(0.0);
            let angle0 = self.ring_angle(&f, rect, press).unwrap_or(0.0);
            self.asm.frame_grip = Some(Held {
                grip,
                start,
                press,
                angle0,
            });
            return true;
        }
        let Some(h) = self.asm.frame_grip else {
            return false;
        };
        if !response.dragged() {
            self.asm.frame_grip = None;
            return false;
        }
        let Some(now) = response.interact_pointer_pos() else {
            return true;
        };
        let fine = ui.input(|i| i.modifiers.shift);
        let value = match h.grip {
            Grip::Rotation => {
                let Some(a) = self.ring_angle(&f, rect, now) else {
                    return true;
                };
                let step = if fine { 1.0 } else { 5.0 };
                let deg = h.start + (a - h.angle0).to_degrees();
                let deg = (deg / step).round() * step;
                (deg + 180.0).rem_euclid(360.0) - 180.0
            }
            g => {
                let Some((arrows, _, mm)) = self.manip_screen(&f, rect) else {
                    return true;
                };
                let Some((_, a, b)) = arrows.iter().find(|x| x.0 == g) else {
                    return true;
                };
                let along = (now - h.press).dot((*b - *a).normalized()) as Real;
                let step = if fine { 0.1 } else { 1.0 };
                ((h.start + along * mm) / step).round() * step
            }
        };
        if let Some(d) = self.part_planned.as_mut() {
            d.set_num(0, h.grip.field(), value);
        }
        true
    }
}

/// Distance from `p` to the segment `a`–`b` on the screen.
fn seg_dist(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = if ab.length_sq() < 1e-6 {
        0.0
    } else {
        ((p - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0)
    };
    p.distance(a + ab * t)
}
