//! **Select Other** inside the open 3D sketch (`docs/spec/1.10-3d-sketch.md`,
//! as in the part, `docs/spec/0.9a-selection-filters.md`): with the cursor
//! resting ~0.7 s where several things a click would take overlap (points of
//! the sketch, its lines, a model vertex, faces behind one another), a small
//! control lists them; the arrows (or Left / Right, Shift + wheel) cycle, the
//! current one is lit and a click at the place (or on its name) takes it.
//! What counts as a candidate follows the running tool (rule 19).

use eframe::egui::{self, Key, Pos2, Rect};

use super::CadCraftApp;
use super::sketch3d::{Hit3, Tool3};
use super::theme;
use crate::kernel::{FaceId, Point3};

/// What the running tool takes from a click.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Want3 {
    /// Lines of the sketch (constraint tools).
    Lines,
    /// A point: of the sketch, a model vertex / work point, or on a face.
    Points,
    /// No tool, or Dimension: points of the sketch, then its lines.
    Select,
    /// Faces of the body (Intersection, Silhouette, Project to Surface).
    Faces,
}

/// One thing under the cursor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Cand3 {
    /// A point of the sketch.
    Own(u32),
    /// A line of the sketch.
    Line(u32),
    /// A model vertex or a work point (its place).
    Model(Point3),
    /// A face of the body, and where the cursor's ray meets it.
    Face(FaceId, Point3),
}

impl Cand3 {
    /// As the hit the 3D sketch's tools read.
    pub(super) fn hit(self) -> Hit3 {
        match self {
            Cand3::Own(id) => Hit3::Own(id),
            Cand3::Line(id) => Hit3::Line(id),
            Cand3::Model(q) => Hit3::At(q, true),
            Cand3::Face(_, q) => Hit3::At(q, false),
        }
    }
}

/// The open Select Other control.
#[derive(Clone, Debug)]
pub(super) struct Sk3Other {
    /// Where the cursor rested.
    pub at: Pos2,
    /// For which kind of pick.
    pub want: Want3,
    /// The candidates, the default first.
    pub items: Vec<Cand3>,
    /// The current one.
    pub index: usize,
}

fn seg_dist(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-9)).clamp(0.0, 1.0);
    (a + ab * t).distance(p)
}

impl CadCraftApp {
    /// What the open 3D sketch's running tool takes; `None` for tools that
    /// pick otherwise (Mirror, Include Geometry, Equation Curve).
    pub(super) fn sk3_want(&self) -> Option<Want3> {
        let e = self.sketch3d.as_ref()?;
        Some(match e.tool {
            None | Some(Tool3::Dimension) => Want3::Select,
            Some(Tool3::Intersection | Tool3::Silhouette) => Want3::Faces,
            Some(Tool3::ProjectSurface) if e.ins.pick_faces => Want3::Faces,
            Some(
                Tool3::Mirror
                | Tool3::Include
                | Tool3::Equation
                | Tool3::ProjectSurface
                | Tool3::AutoBend,
            ) => return None,
            Some(t) if t.picks_lines() => Want3::Lines,
            Some(_) => Want3::Points,
        })
    }

    /// Every candidate under `p` for `want`, the default (nearest, in pick
    /// order) first.
    pub(super) fn sk3_candidates(&self, p: Pos2, rect: Rect, want: Want3) -> Vec<Cand3> {
        let Some(e) = self.sketch3d.as_ref() else {
            return Vec::new();
        };
        let Some(s) = self.part().sketches3d.get(e.index) else {
            return Vec::new();
        };
        let screen = |q: Point3| self.camera.project(&q, rect);
        let mut points: Vec<(f32, u32)> = s
            .points
            .iter()
            .filter_map(|q| Some((screen(Point3::from(q.at))?.distance(p), q.id)))
            .filter(|(d, _)| *d <= super::overlay::POINT_SNAP_PX)
            .collect();
        points.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut lines: Vec<(f32, u32)> = s
            .lines
            .iter()
            .filter_map(|l| Some((seg_dist(p, screen(s.at(l.a)?)?, screen(s.at(l.b)?)?), l.id)))
            .filter(|(d, _)| *d <= super::overlay::CURVE_SNAP_PX)
            .collect();
        lines.sort_by(|a, b| a.0.total_cmp(&b.0));
        let faces = || -> Vec<Cand3> {
            let Some((o, d)) = self.camera.ray(p, rect) else {
                return Vec::new();
            };
            let Some(d) = d.try_normalize(1e-12) else {
                return Vec::new();
            };
            self.picker()
                .hits(&o, &d)
                .into_iter()
                .map(|(f, t)| Cand3::Face(f, o + d * t))
                .collect()
        };
        let mut out: Vec<Cand3> = Vec::new();
        match want {
            Want3::Lines => out.extend(lines.iter().map(|(_, id)| Cand3::Line(*id))),
            Want3::Select => {
                out.extend(points.iter().map(|(_, id)| Cand3::Own(*id)));
                out.extend(lines.iter().map(|(_, id)| Cand3::Line(*id)));
            }
            Want3::Faces => out.extend(faces()),
            Want3::Points => {
                out.extend(points.iter().map(|(_, id)| Cand3::Own(*id)));
                if let Some(r) = match self.work_axis_or_point_under(p, rect) {
                    Some(super::work::WpInput::Point(r)) => Some(r),
                    _ => self.vertex_under(p, rect),
                } && let Ok(q) =
                    crate::part::placement::point_of(&r, &self.part().body, &self.part().work)
                {
                    out.push(Cand3::Model(q));
                }
                out.extend(faces());
            }
        }
        out
    }

    /// The candidate Select Other has chosen for a pick at `p`, when its
    /// control is open there for `want`.
    pub(super) fn sk3_other_choice(&self, p: Pos2, want: Want3) -> Option<Cand3> {
        let o = self.sketch3d.as_ref()?.other.as_ref()?;
        (o.want == want && o.at.distance(p) <= 3.0)
            .then(|| o.items.get(o.index).copied())
            .flatten()
    }

    /// The pick at `p` for `want`: Select Other's choice, else the default.
    pub(super) fn sk3_pick(&self, p: Pos2, rect: Rect, want: Want3) -> Option<Cand3> {
        self.sk3_other_choice(p, want)
            .or_else(|| self.sk3_candidates(p, rect, want).first().copied())
    }

    /// A click was taken: the control closes.
    pub(super) fn sk3_other_close(&mut self) {
        if let Some(e) = self.sketch3d.as_mut() {
            e.other = None;
            e.rest = None;
        }
    }

    fn sk3_cand_label(&self, c: &Cand3) -> String {
        let Some(e) = self.sketch3d.as_ref() else {
            return String::new();
        };
        let s = self.part().sketches3d.get(e.index);
        let xyz = |q: Point3| format!("({:.2}, {:.2}, {:.2})", q.x, q.y, q.z);
        match *c {
            Cand3::Own(id) => s
                .and_then(|s| s.at(id))
                .map_or_else(|| "Point".into(), |q| format!("3D Sketch Point {}", xyz(q))),
            Cand3::Line(id) => s
                .and_then(|s| {
                    let l = s.line(id)?;
                    Some((s.at(l.b)? - s.at(l.a)?).norm())
                })
                .map_or_else(
                    || "Line".into(),
                    |len| format!("3D Sketch Line  ·  L {len:.2} mm"),
                ),
            Cand3::Model(q) => format!("Vertex {}", xyz(q)),
            Cand3::Face(f, _) => {
                let name = self.part().body.face(f).and_then(|x| x.name.clone());
                match name {
                    Some(n) if self.internal_names => format!("Face {n}"),
                    Some(n) => {
                        let feature = self
                            .face_feature(&n)
                            .map(|k| format!("  ·  {}", self.part().features[k].name()))
                            .unwrap_or_default();
                        format!("Face{feature}")
                    }
                    None => "Face".into(),
                }
            }
        }
    }

    /// Once a frame while the 3D sketch is open: the cursor's rest opens the
    /// control over several candidates, which is drawn and cycled here.
    pub(super) fn sk3_other_ui(&mut self, ui: &mut egui::Ui, rect: Rect, pointer: Option<Pos2>) {
        let Some(want) = self.sk3_want() else {
            self.sk3_other_close();
            return;
        };
        let now = ui.input(|i| i.time);
        let rest = self.sketch3d.as_ref().and_then(|e| e.rest);
        match (pointer, rest) {
            (Some(p), Some((at, _))) if p.distance(at) <= 3.0 => {}
            (Some(p), _) => {
                if let Some(e) = self.sketch3d.as_mut() {
                    e.rest = Some((p, now));
                    if e.other
                        .as_ref()
                        .is_some_and(|o| o.at.distance(p) > 60.0 || o.want != want)
                    {
                        e.other = None;
                    }
                }
            }
            (None, _) => {}
        }
        let open = self.sketch3d.as_ref().is_some_and(|e| e.other.is_some());
        // A frame when the rest is long enough (egui paints on input only).
        if !open
            && let Some((_, since)) = self.sketch3d.as_ref().and_then(|e| e.rest)
            && since.is_finite()
        {
            let wait = (since + 0.75 - now).max(0.0);
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f64(wait));
        }
        if !open
            && self.dragging.is_none()
            && let (Some(p), Some((at, since))) =
                (pointer, self.sketch3d.as_ref().and_then(|e| e.rest))
            && rect.contains(p)
            && p.distance(at) <= 3.0
            && now - since > 0.7
        {
            let items = self.sk3_candidates(p, rect, want);
            if let Some(e) = self.sketch3d.as_mut() {
                // Asked once per rest: a single candidate needs no control.
                e.rest = Some((at, f64::INFINITY));
                if items.len() > 1 {
                    e.other = Some(Sk3Other {
                        at: p,
                        want,
                        items,
                        index: 0,
                    });
                }
            }
        }
        let Some(o) = self.sketch3d.as_ref().and_then(|e| e.other.clone()) else {
            return;
        };
        ui.ctx().request_repaint();
        let n = o.items.len();
        let label = self.sk3_cand_label(&o.items[o.index]);
        let pos = egui::pos2(
            (o.at.x + 14.0).min(rect.right() - 260.0),
            (o.at.y - 40.0).max(rect.top() + 4.0),
        );
        let mut step = 0i32;
        let mut accept = false;
        egui::Area::new(egui::Id::new("sk3_select_other"))
            .fixed_pos(pos)
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style())
                    .inner_margin(3.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if theme::glyph(ui, theme::Glyph::Left)
                                .on_hover_text("Previous (Left)")
                                .clicked()
                            {
                                step = -1;
                            }
                            if theme::glyph(ui, theme::Glyph::Right)
                                .on_hover_text("Next (Right)")
                                .clicked()
                            {
                                step = 1;
                            }
                            if ui
                                .selectable_label(true, format!("{} / {n}  {label}", o.index + 1))
                                .on_hover_text("Click to take it")
                                .clicked()
                            {
                                accept = true;
                            }
                        });
                    });
            });
        let (left, right, shift, wheel) = ui.input(|i| {
            let d: egui::Vec2 = i
                .events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::MouseWheel { delta, .. } => Some(*delta),
                    _ => None,
                })
                .fold(egui::Vec2::ZERO, |a, b| a + b);
            (
                i.key_pressed(Key::ArrowLeft) || i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::ArrowRight) || i.key_pressed(Key::ArrowDown),
                i.modifiers.shift,
                if d.y != 0.0 { d.y } else { d.x },
            )
        });
        if left {
            step = -1;
        }
        if right {
            step = 1;
        }
        if shift && wheel != 0.0 {
            step = if wheel > 0.0 { -1 } else { 1 };
        }
        if step != 0
            && let Some(x) = self.sketch3d.as_mut().and_then(|e| e.other.as_mut())
        {
            x.index = (x.index as i32 + step).rem_euclid(n as i32) as usize;
        }
        if accept {
            // As if the place were clicked: the tool takes the choice.
            self.inject_click = Some(o.at);
        }
    }

    /// Draws the current Select Other candidate (a point ring or a thick
    /// line; faces are lit in the scene through [`Self::sk3_hover_face`]).
    pub(super) fn sk3_other_draw(&self, painter: &egui::Painter, rect: Rect) {
        let Some(e) = self.sketch3d.as_ref() else {
            return;
        };
        let Some(o) = e.other.as_ref() else { return };
        let Some(s) = self.part().sketches3d.get(e.index) else {
            return;
        };
        let hover = theme::canvas().hover_edge;
        let ring = |q: Point3| {
            if let Some(sp) = self.camera.project(&q, rect) {
                painter.circle_stroke(sp, 7.0, egui::Stroke::new(2.5, hover));
            }
        };
        match o.items.get(o.index) {
            Some(Cand3::Own(id)) => {
                if let Some(q) = s.at(*id) {
                    ring(q);
                }
            }
            Some(Cand3::Model(q) | Cand3::Face(_, q)) => ring(*q),
            Some(Cand3::Line(id)) => {
                if let Some(l) = s.line(*id)
                    && let (Some(a), Some(b)) = (s.at(l.a), s.at(l.b))
                    && let (Some(a), Some(b)) =
                        (self.camera.project(&a, rect), self.camera.project(&b, rect))
                {
                    painter.line_segment([a, b], egui::Stroke::new(5.0, hover));
                }
            }
            _ => {}
        }
    }

    /// The face a Faces tool would take under the cursor (Select Other's
    /// choice first), for the scene's highlight.
    pub(super) fn sk3_hover_face(&self, pointer: Option<Pos2>, rect: Rect) -> Option<FaceId> {
        let p = pointer?;
        // Select Other's current face is lit whatever the tool takes.
        if let Some(o) = self.sketch3d.as_ref().and_then(|e| e.other.as_ref())
            && let Some(Cand3::Face(f, _)) = o.items.get(o.index)
        {
            return Some(*f);
        }
        if self.sk3_want()? != Want3::Faces {
            return None;
        }
        match self.sk3_pick(p, rect, Want3::Faces)? {
            Cand3::Face(f, _) => Some(f),
            _ => None,
        }
    }
}
