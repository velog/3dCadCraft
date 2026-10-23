//! Measure into a field (`docs/spec/0-ui.md` §2.7, the `>` menu's
//! *Measure*; Inventor's "Measure" of an edit box): while it runs, clicks
//! in the view pick sketch points and curves of the edited sketch or model
//! vertices, edges and faces (two at most, a third starts again), and a
//! small window lists what they give — one item: a line's length, a
//! circle's diameter and radius, an arc's length, a face's area; two: the
//! distance (point–point, point–line, parallel lines, centres) or the
//! angle of two lines. A click on a value puts it into the field the
//! measure started from: the in-place sketch dimension box or any
//! expression field (`valuefield`). Esc or Cancel ends it, the field keeps
//! its value.

use eframe::egui::{self, Color32, RichText, Stroke};

use super::CadCraftApp;
use super::overlay;
use super::select::SelItem;
use crate::kernel::{Point3, Real, Vector3};
use crate::part::expr::Unit;
use crate::part::measure::{self, Target};
use crate::sketch::Entity;

/// The field a measure fills.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum MeasureInto {
    /// The in-place Edit Dimension box of the edited sketch.
    DimEdit,
    /// An expression field (`valuefield::value_field`) by its id and unit.
    Field(egui::Id, Unit),
}

/// One picked item.
#[derive(Clone, Debug)]
pub(super) enum MPick {
    /// A point (sketch point, model vertex).
    Point(Point3),
    /// A straight segment.
    Line(Point3, Point3),
    /// A circle or an arc: centre, radius, sweep (2π for a circle).
    Circle(Point3, Real, Real),
    /// A model edge / face (measured by `part::measure`).
    Model(Target),
}

/// The running measure.
#[derive(Clone, Debug)]
pub(super) struct MeasurePick {
    pub into: MeasureInto,
    pub picks: Vec<MPick>,
}

/// Temp-data key a field uses to ask for a measure.
pub(super) fn request_id() -> egui::Id {
    egui::Id::new("measure_into_request")
}

/// Temp-data key of a measured value for field `id`.
pub(super) fn result_id(id: egui::Id) -> egui::Id {
    id.with("measured")
}

/// A value as typed into a field of `unit`.
fn text(v: Real, angle: bool, unit: Option<Unit>) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.').to_owned();
    match (angle, unit) {
        (true, _) => format!("{s} deg"),
        (false, Some(u)) if u.is_angle() || u == Unit::Ul => s,
        (false, _) => format!("{s} mm"),
    }
}

fn dist_point_line(p: Point3, a: Point3, b: Point3) -> Real {
    let d = b - a;
    let t = (p - a).dot(&d) / d.norm_squared().max(1e-300);
    (p - (a + d * t)).norm()
}

/// What the picks measure: (label, value, is an angle in degrees).
pub(super) fn results(body: &crate::kernel::BRep, picks: &[MPick]) -> Vec<(String, Real, bool)> {
    let mut out = Vec::new();
    let centre = |p: &MPick| match p {
        MPick::Point(q) => Some(*q),
        MPick::Circle(c, ..) => Some(*c),
        _ => None,
    };
    match picks {
        [one] => match one {
            MPick::Line(a, b) => out.push(("Length".into(), (b - a).norm(), false)),
            MPick::Circle(_, r, sweep) => {
                out.push(("Diameter".into(), 2.0 * r, false));
                out.push(("Radius".into(), *r, false));
                if *sweep < std::f64::consts::TAU - 1e-9 {
                    out.push(("Arc length".into(), r * sweep, false));
                }
            }
            MPick::Model(t) => {
                if let Ok(m) = measure::measure(body, std::slice::from_ref(t))
                    && let Some(it) = m.items.first()
                {
                    if let Some(r) = it.radius {
                        out.push(("Diameter".into(), 2.0 * r, false));
                        out.push(("Radius".into(), r, false));
                    }
                    if let Some(l) = it.length {
                        out.push(("Length".into(), l, false));
                    }
                    if let Some(a) = it.area {
                        out.push(("Area (mm²)".into(), a, false));
                    }
                }
            }
            MPick::Point(_) => {}
        },
        [a, b] => {
            match (a, b) {
                (MPick::Line(p, q), MPick::Line(r, s)) => {
                    let (u, v) = (q - p, s - r);
                    let c = (u.dot(&v) / (u.norm() * v.norm()).max(1e-300)).clamp(-1.0, 1.0);
                    if u.cross(&v).norm() <= 1e-9 * u.norm() * v.norm() {
                        out.push(("Distance".into(), dist_point_line(*r, *p, *q), false));
                    } else {
                        let ang = c.acos().to_degrees();
                        out.push(("Angle".into(), ang, true));
                        out.push(("Angle (supplement)".into(), 180.0 - ang, true));
                    }
                }
                (MPick::Line(p, q), x) | (x, MPick::Line(p, q)) if centre(x).is_some() => {
                    if let Some(c) = centre(x) {
                        out.push(("Distance".into(), dist_point_line(c, *p, *q), false));
                    }
                }
                (MPick::Model(x), MPick::Model(y)) => {
                    if let Ok(m) = measure::measure(body, &[x.clone(), y.clone()]) {
                        if let Some((d, ..)) = m.distance {
                            out.push(("Distance".into(), d, false));
                        }
                        if let Some(c) = m.center_distance {
                            out.push(("Centre distance".into(), c, false));
                        }
                        if let Some(a) = m.angle_deg {
                            out.push(("Angle".into(), a, true));
                        }
                    }
                }
                (MPick::Model(t), x) | (x, MPick::Model(t)) if centre(x).is_some() => {
                    if let Some(c) = centre(x)
                        && let Ok(m) = measure::measure(body, &[t.clone(), Target::Point(c)])
                        && let Some((d, ..)) = m.distance
                    {
                        out.push(("Distance".into(), d, false));
                    }
                }
                _ => {}
            }
            if let (Some(p), Some(q)) = (centre(a), centre(b)) {
                let label = if matches!(a, MPick::Circle(..)) || matches!(b, MPick::Circle(..)) {
                    "Centre distance"
                } else {
                    "Distance"
                };
                out.push((label.into(), (q - p).norm(), false));
            }
        }
        _ => {}
    }
    out
}

impl CadCraftApp {
    /// Starts a measure for `into` (the `>` menu, the dimension box).
    pub(super) fn start_measure_into(&mut self, into: MeasureInto) {
        self.measure_pick = Some(MeasurePick {
            into,
            picks: Vec::new(),
        });
        self.file_status = None;
    }

    /// A field's `>` → Measure asked for one this frame.
    pub(super) fn measure_requests(&mut self, ctx: &egui::Context) {
        let req = ctx.data_mut(|d| {
            let r = d.get_temp::<(egui::Id, Unit)>(request_id());
            d.remove::<(egui::Id, Unit)>(request_id());
            r
        });
        if let Some((id, unit)) = req {
            self.start_measure_into(MeasureInto::Field(id, unit));
        }
    }

    /// What a click at `p` picks for a measure.
    fn measure_item_at(&self, p: egui::Pos2, rect: egui::Rect) -> Option<MPick> {
        if let Some(i) = self.editing {
            let sk = &self.part().sketches[i].sketch;
            if let Some(q) =
                overlay::pick_any_point(&self.camera, rect, sk, p, overlay::POINT_PICK_PX)
                && let Some(pt) = sk.point(q)
            {
                return Some(MPick::Point(sk.to_world(&pt.position)));
            }
            if let Some((e, _)) =
                overlay::pick_entity(&self.camera, rect, sk, p, overlay::CURVE_SNAP_PX)
            {
                let at = |id| sk.point(id).map(|x| sk.to_world(&x.position));
                match sk.entity(e)? {
                    Entity::Line { start, end } => {
                        return Some(MPick::Line(at(*start)?, at(*end)?));
                    }
                    Entity::Circle { center, radius } => {
                        return Some(MPick::Circle(at(*center)?, *radius, std::f64::consts::TAU));
                    }
                    Entity::Arc { center, start, end } => {
                        let (c, s, t) = (at(*center)?, at(*start)?, at(*end)?);
                        let n: Vector3 = sk.plane.normal.into_inner();
                        let (u, v) = (s - c, t - c);
                        let mut sweep = n.dot(&u.cross(&v)).atan2(u.dot(&v));
                        if sweep <= 0.0 {
                            sweep += std::f64::consts::TAU;
                        }
                        return Some(MPick::Circle(c, u.norm(), sweep));
                    }
                    Entity::Spline { .. } => {}
                }
            }
        }
        let edge = self.edge_under(p, rect);
        let face = match self.plane_under(p, rect, false) {
            Some(crate::part::PlaneRef::Face(n)) => Some(n),
            _ => None,
        };
        match self.sel_item_at(p, rect, edge, face.as_deref())? {
            SelItem::Vertex(n) => {
                let v = self
                    .part()
                    .body
                    .vertices()
                    .find(|(_, v)| v.name.as_deref() == Some(n.as_str()))?;
                Some(MPick::Point(v.1.point))
            }
            SelItem::Edge(e) => Some(MPick::Model(Target::Edge(e))),
            SelItem::Face(n) => Some(MPick::Model(Target::Face(n))),
            _ => None,
        }
    }

    /// A click in the view while measuring: the item, and the click is
    /// taken from the open command (it must not pick there too).
    pub(super) fn measure_pick_click(&mut self, response: &mut egui::Response, rect: egui::Rect) {
        if self.measure_pick.is_none() || !response.clicked_by(egui::PointerButton::Primary) {
            return;
        }
        let Some(p) = response.interact_pointer_pos() else {
            return;
        };
        let item = self.measure_item_at(p, rect);
        response
            .flags
            .remove(egui::response::Flags::CLICKED | egui::response::Flags::FAKE_PRIMARY_CLICKED);
        let (Some(m), Some(item)) = (self.measure_pick.as_mut(), item) else {
            return;
        };
        if m.picks.len() >= 2 {
            m.picks.clear();
        }
        m.picks.push(item);
    }

    /// The picked items in the view.
    pub(super) fn draw_measure_pick(&self, painter: &egui::Painter, rect: egui::Rect) {
        let Some(m) = &self.measure_pick else { return };
        let col = Color32::from_rgb(230, 120, 20);
        let pt = |p: &Point3| self.camera.project(p, rect);
        for it in &m.picks {
            match it {
                MPick::Point(p) => {
                    if let Some(s) = pt(p) {
                        painter.circle_filled(s, 5.0, col);
                    }
                }
                MPick::Line(a, b) => {
                    if let (Some(a), Some(b)) = (pt(a), pt(b)) {
                        painter.line_segment([a, b], Stroke::new(4.0, col));
                    }
                }
                MPick::Circle(c, ..) => {
                    if let Some(s) = pt(c) {
                        painter.circle_stroke(s, 6.0, Stroke::new(2.5, col));
                    }
                }
                MPick::Model(Target::Edge(e)) => {
                    for (id, pts) in self.edge_polylines().iter() {
                        if id == e {
                            let s: Vec<_> = pts.iter().filter_map(pt).collect();
                            painter.add(egui::Shape::line(s, Stroke::new(4.0, col)));
                        }
                    }
                }
                MPick::Model(_) => {}
            }
        }
    }

    /// The window with the values; a click on one fills the field.
    pub(super) fn measure_pick_window(&mut self, ctx: &egui::Context) {
        let Some(m) = self.measure_pick.clone() else {
            return;
        };
        let unit = match m.into {
            MeasureInto::Field(_, u) => Some(u),
            MeasureInto::DimEdit => None,
        };
        let res = results(&self.part().body, &m.picks);
        let mut chosen: Option<String> = None;
        let mut cancel = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        let mut open = true;
        egui::Window::new("Measure into field")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos(self.viewport_rect.right_top() + egui::vec2(-300.0, 20.0))
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(
                        "Click two points, a line, a circle, an edge or a face; then the \
                         value to use.",
                    )
                    .weak(),
                );
                ui.label(format!("{} item(s) picked", m.picks.len()));
                ui.separator();
                if res.is_empty() {
                    ui.label(RichText::new("Nothing measured yet").weak());
                }
                for (label, v, angle) in &res {
                    let shown = if *angle {
                        format!("{label}: {v:.3}°")
                    } else {
                        format!("{label}: {v:.3}")
                    };
                    if ui
                        .button(shown)
                        .on_hover_text("Put this value into the field")
                        .clicked()
                    {
                        chosen = Some(text(*v, *angle, unit));
                    }
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Clear").clicked()
                        && let Some(mp) = self.measure_pick.as_mut()
                    {
                        mp.picks.clear();
                    }
                    cancel |= ui.button("Cancel").clicked();
                });
            });
        if let Some(t) = chosen {
            match m.into {
                MeasureInto::DimEdit => {
                    if let Some(e) = self.tools.dim_edit.as_mut() {
                        // An angle box takes plain degrees.
                        e.text = t
                            .trim_end_matches(" deg")
                            .trim_end_matches(" mm")
                            .to_owned();
                        e.focused = false;
                    }
                }
                MeasureInto::Field(id, _) => {
                    ctx.data_mut(|d| d.insert_temp(result_id(id), t));
                }
            }
            self.measure_pick = None;
        } else if cancel || !open {
            self.measure_pick = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_of_sketch_picks() {
        let body = crate::kernel::BRep::default();
        let l = MPick::Line(Point3::new(0.0, 0.0, 0.0), Point3::new(3.0, 4.0, 0.0));
        assert_eq!(results(&body, std::slice::from_ref(&l))[0].1, 5.0);
        let c = MPick::Circle(Point3::new(10.0, 0.0, 0.0), 2.5, std::f64::consts::TAU);
        let r = results(&body, std::slice::from_ref(&c));
        assert_eq!((r[0].0.as_str(), r[0].1), ("Diameter", 5.0));
        assert_eq!(r.len(), 2, "no arc length for a full circle");
        let p = MPick::Point(Point3::new(0.0, 3.0, 0.0));
        let q = MPick::Point(Point3::new(4.0, 0.0, 0.0));
        assert_eq!(results(&body, &[p.clone(), q])[0].1, 5.0);
        // Point to line, circle centre to line.
        let h = MPick::Line(Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0));
        assert!((results(&body, &[p, h.clone()])[0].1 - 3.0).abs() < 1e-12);
        // Two lines at 90°.
        let v = MPick::Line(Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 5.0, 0.0));
        let a = results(&body, &[h.clone(), v]);
        assert!(a[0].2 && (a[0].1 - 90.0).abs() < 1e-9);
        // Parallel lines: their distance.
        let h2 = MPick::Line(Point3::new(0.0, 7.0, 0.0), Point3::new(5.0, 7.0, 0.0));
        assert!((results(&body, &[h, h2])[0].1 - 7.0).abs() < 1e-12);
        assert_eq!(text(12.5, false, Some(Unit::Mm)), "12.5 mm");
        assert_eq!(text(30.0, true, None), "30 deg");
    }
}
