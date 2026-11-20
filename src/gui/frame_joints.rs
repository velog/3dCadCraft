//! The joints of a frame in the view (spec 6.1 §3.14, owner's batch
//! 2026-11-20): with a member of a frame selected (or its Edit Frame Members
//! open) every joint of that frame shows an icon of its treatment — Miter,
//! Corner Joint (butt), Notch, or an amber ring where the members meet uncut.
//! A click opens the joint's panel: the treatments that joint can take and a
//! Gap; each choice is applied at once as a preview (the members rebuilt and
//! drawn), OK keeps it as one Undo step, Cancel / Esc / a click elsewhere puts
//! the frame back. The joints are found once per frame state (rule 20).

use super::CadCraftApp;
use super::valuefield::ExprValue;
use crate::assembly::frame::{self, Joint, JointKind, JointType};
use crate::document::Project;
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};
use std::cell::RefCell;
use std::rc::Rc;

/// Half the size of a joint's icon, px.
const HALF: f32 = 8.0;

/// The open joint panel.
pub(super) struct JointEdit {
    /// The joint (frame assembly ids).
    pub joint: Joint,
    /// Where the panel stands on the screen.
    pub at: Pos2,
    /// The treatment chosen in the panel.
    pub choice: JointType,
    /// Its gap, mm.
    pub gap: f64,
    /// The project before the preview (`None`: nothing applied yet).
    pub before: Option<Box<Project>>,
    /// Why the last preview failed.
    pub error: Option<String>,
    /// The click that opened the panel is still being handled.
    pub fresh: bool,
}

thread_local! {
    /// The joints of the frame assembly by a key of its state.
    static JOINTS: RefCell<Option<(u64, Rc<Vec<Joint>>)>> = const { RefCell::new(None) };
}

/// The icon's colours: treated joints blue, uncut amber.
fn colour(t: JointType) -> Color32 {
    match t {
        JointType::None => Color32::from_rgb(255, 170, 30),
        _ => Color32::from_rgb(70, 150, 255),
    }
}

/// The treatment's name in the panel (`a`, `b`: the members' names).
fn choice_label(t: JointType, kind: JointKind, a: &str, b: &str) -> String {
    match (t, kind) {
        (JointType::Miter, _) => "Miter".into(),
        (JointType::FirstThrough, JointKind::Corner) => {
            format!("Butt: {a} runs through ({b} trimmed / extended)")
        }
        (JointType::FirstThrough, JointKind::Tee) => {
            format!("Butt: {b} trimmed / extended to {a}")
        }
        (JointType::SecondThrough, _) => {
            format!("Butt: {b} runs through ({a} trimmed / extended)")
        }
        (JointType::Notch, _) => format!("Notch: {b} notched by {a}"),
        (JointType::None, _) => "None (uncut)".into(),
    }
}

/// Short name of a treatment (the icon's hover text).
fn short(t: JointType) -> &'static str {
    match t {
        JointType::Miter => "Miter",
        JointType::FirstThrough | JointType::SecondThrough => "Corner Joint",
        JointType::Notch => "Notch",
        JointType::None => "uncut",
    }
}

/// Draws a joint's icon at `q`: a mitre's diagonal, a butt's two bars, a
/// notch's cut-out, an uncut joint's ring.
fn paint_icon(painter: &egui::Painter, q: Pos2, t: JointType, hot: bool) {
    let r = Rect::from_center_size(q, Vec2::splat(2.0 * HALF));
    let fill = if hot {
        crate::gui::theme::hover_edge()
    } else {
        colour(t)
    };
    let ink = Stroke::new(1.6, Color32::WHITE);
    if t == JointType::None {
        painter.circle(
            q,
            HALF - 1.0,
            Color32::from_black_alpha(90),
            Stroke::new(2.5, fill),
        );
        painter.circle_filled(q, 2.0, fill);
        return;
    }
    painter.rect_filled(r, 3.0, fill);
    painter.rect_stroke(
        r,
        3.0,
        Stroke::new(1.0, Color32::from_black_alpha(160)),
        egui::StrokeKind::Outside,
    );
    let i = r.shrink(3.5);
    match t {
        JointType::Miter => {
            // Two bars meeting on the diagonal.
            painter.line_segment([i.left_top(), i.right_top()], ink);
            painter.line_segment([i.right_top(), i.right_bottom()], ink);
            painter.line_segment([i.left_top(), i.right_bottom()], ink);
        }
        JointType::FirstThrough | JointType::SecondThrough => {
            // One bar through, the other stopping at it.
            painter.line_segment([i.left_top(), i.right_top()], ink);
            painter.line_segment(
                [
                    i.left_top() + Vec2::new(0.0, 3.0),
                    i.right_top() + Vec2::new(0.0, 3.0),
                ],
                ink,
            );
            let x = i.center().x;
            painter.line_segment(
                [
                    Pos2::new(x - 2.0, i.top() + 3.0),
                    Pos2::new(x - 2.0, i.bottom()),
                ],
                ink,
            );
            painter.line_segment(
                [
                    Pos2::new(x + 2.0, i.top() + 3.0),
                    Pos2::new(x + 2.0, i.bottom()),
                ],
                ink,
            );
        }
        JointType::Notch => {
            // A bar with a cut-out where the other passes.
            let y = i.center().y;
            painter.line_segment([i.left_bottom(), i.right_bottom()], ink);
            painter.line_segment(
                [Pos2::new(i.left(), y), Pos2::new(i.center().x - 3.0, y)],
                ink,
            );
            painter.line_segment(
                [Pos2::new(i.center().x + 3.0, y), Pos2::new(i.right(), y)],
                ink,
            );
            painter.line_segment(
                [
                    Pos2::new(i.center().x - 3.0, y),
                    Pos2::new(i.center().x - 3.0, i.top()),
                ],
                ink,
            );
            painter.line_segment(
                [
                    Pos2::new(i.center().x + 3.0, y),
                    Pos2::new(i.center().x + 3.0, i.top()),
                ],
                ink,
            );
        }
        JointType::None => {}
    }
}

impl CadCraftApp {
    /// Every joint of the frame assembly worked on, kept until a member,
    /// a placement or a treatment changes.
    fn frame_joint_list(&self) -> Rc<Vec<Joint>> {
        use std::hash::{Hash, Hasher};
        let Some(a) = self.frame_asm() else {
            return Rc::default();
        };
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.frame_doc().hash(&mut h);
        for o in &a.occurrences {
            o.id.hash(&mut h);
            for v in o
                .transform
                .translation
                .vector
                .iter()
                .chain(o.transform.rotation.coords.iter())
            {
                v.to_bits().hash(&mut h);
            }
        }
        for f in &a.frames {
            f.id.hash(&mut h);
            format!(
                "{:?}{:?}",
                f.treatments,
                f.members
                    .iter()
                    .map(|m| (&m.skeleton, &m.section))
                    .collect::<Vec<_>>()
            )
            .hash(&mut h);
        }
        let key = h.finish();
        if let Some(v) = JOINTS.with(|c| {
            c.borrow()
                .as_ref()
                .filter(|(k, _)| *k == key)
                .map(|(_, v)| v.clone())
        }) {
            return v;
        }
        let v = Rc::new(frame::joints(&self.project, &self.frame_linked(), a));
        JOINTS.with(|c| *c.borrow_mut() = Some((key, v.clone())));
        v
    }

    /// The frames whose joints are shown: those of a selected member, of the
    /// members Edit Frame Members works on, of the open joint panel.
    fn joint_frames(&self) -> Vec<u32> {
        let Some(a) = self.frame_asm() else {
            return Vec::new();
        };
        let mut members = self.frame_selected();
        if let Some(d) = self.part_planned.as_ref() {
            if d.members.list.is_empty() {
                return Vec::new();
            }
            members.extend(&d.members.list);
        }
        let mut out: Vec<u32> = a
            .frames
            .iter()
            .filter(|f| f.members.iter().any(|m| members.contains(&m.occurrence)))
            .map(|f| f.id)
            .collect();
        if let Some(e) = &self.asm.corner_menu
            && !out.contains(&e.joint.frame)
        {
            out.push(e.joint.frame);
        }
        out
    }

    /// The icons on the screen: the joint and where.
    fn joint_icons(&self, rect: Rect) -> Vec<(Joint, Pos2)> {
        let frames = self.joint_frames();
        if frames.is_empty() {
            return Vec::new();
        }
        let place = self.frame_place();
        let mut out: Vec<(Joint, Pos2)> = Vec::new();
        for j in self.frame_joint_list().iter() {
            if !frames.contains(&j.frame) {
                continue;
            }
            let Some(mut q) = self.camera.project(&(place * j.at), rect) else {
                continue;
            };
            if !rect.contains(q) {
                continue;
            }
            // Joints at one point (a corner of three): side by side.
            while out.iter().any(|(_, p)| p.distance(q) < 2.0 * HALF) {
                q.x += 2.4 * HALF;
            }
            out.push((j.clone(), q));
        }
        out
    }

    /// Draws the joints' icons; the one under the cursor (or open) is lit,
    /// with what it is beside it.
    pub(super) fn frame_corner_overlay(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        pointer: Option<Pos2>,
    ) {
        let open = self.asm.corner_menu.as_ref().map(|e| e.joint.members);
        for (j, q) in self.joint_icons(rect) {
            let under = pointer.is_some_and(|p| p.distance(q) <= HALF + 2.0);
            let shown = match &self.asm.corner_menu {
                Some(e) if Some(j.members) == open => e.choice,
                _ => j.current,
            };
            paint_icon(painter, q, shown, under || Some(j.members) == open);
            if under && open.is_none() {
                let kind = match j.kind {
                    JointKind::Corner => "Corner",
                    JointKind::Tee => "T joint",
                };
                painter.text(
                    q + Vec2::new(HALF + 4.0, -HALF),
                    egui::Align2::LEFT_BOTTOM,
                    format!("{kind}: {} · click to change", short(j.current)),
                    egui::FontId::proportional(12.0),
                    crate::gui::theme::hover_edge(),
                );
            }
        }
    }

    /// A click on a joint's icon opens its panel (`true`: taken).
    pub(super) fn frame_corner_click(&mut self, p: Pos2, rect: Rect) -> bool {
        let hit = self
            .joint_icons(rect)
            .into_iter()
            .find(|(_, q)| q.distance(p) <= HALF + 2.0);
        let Some((joint, q)) = hit else {
            return false;
        };
        self.joint_cancel();
        self.asm.corner_menu = Some(JointEdit {
            choice: joint.current,
            gap: joint.gap,
            joint,
            at: q + Vec2::new(HALF + 6.0, HALF + 6.0),
            before: None,
            error: None,
            fresh: true,
        });
        true
    }

    /// Puts the frame back as it was before the open joint panel's preview
    /// and closes the panel.
    pub(super) fn joint_cancel(&mut self) {
        if let Some(e) = self.asm.corner_menu.take()
            && let Some(b) = e.before
        {
            self.project = *b;
            self.scene_dirty = true;
        }
    }

    /// Applies the panel's choice to the project as the preview (from the
    /// state before the panel opened).
    fn joint_preview(&mut self) {
        let Some(mut e) = self.asm.corner_menu.take() else {
            return;
        };
        if let Some(b) = e.before.take() {
            self.project = *b;
        }
        let before = self.project.clone();
        let linked = self.frame_linked();
        let doc = self.frame_doc();
        e.error = None;
        if e.choice == e.joint.current && (e.gap - e.joint.gap).abs() < 1e-9 {
            // Back to what it was: no preview.
        } else {
            match frame::set_joint(
                &mut self.project,
                &linked,
                doc,
                e.joint.frame,
                e.joint.members,
                e.choice,
                e.gap,
            ) {
                Ok(_) => e.before = Some(Box::new(before)),
                Err(err) => e.error = Some(err),
            }
        }
        self.scene_dirty = true;
        self.asm.corner_menu = Some(e);
    }

    /// OK of the joint panel: the preview becomes one Undo step.
    fn joint_ok(&mut self) {
        let Some(e) = self.asm.corner_menu.take() else {
            return;
        };
        if e.error.is_some() {
            self.file_status = Some(Err(format!("Frame Joint: {}", e.error.unwrap_or_default())));
            if let Some(b) = e.before {
                self.project = *b;
                self.scene_dirty = true;
            }
            return;
        }
        let Some(b) = e.before else {
            return;
        };
        let trial = std::mem::replace(&mut self.project, *b);
        self.frame_commit("Frame Joint", trial);
    }

    /// The open joint panel: the treatments the joint can take, its Gap, the
    /// preview's error, OK / Cancel. Enter = OK, Esc or a click elsewhere =
    /// Cancel.
    pub(super) fn frame_corner_menu(&mut self, ctx: &egui::Context) {
        let Some(e) = self.asm.corner_menu.as_ref() else {
            return;
        };
        let name = |id: u32| {
            self.frame_label(id)
                .map(|n| super::frame_select::pick_name(&n).to_owned())
                .unwrap_or_else(|| format!("member {id}"))
        };
        let (a, b) = (name(e.joint.members[0]), name(e.joint.members[1]));
        let (at, kind, fresh) = (e.at, e.joint.kind, e.fresh);
        let mut choice = e.choice;
        let mut gap = e.gap;
        let error = e.error.clone();
        let (mut ok, mut cancel) = (false, false);
        let r = egui::Area::new(egui::Id::new("frame_corner_menu"))
            .fixed_pos(at)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_max_width(320.0);
                    ui.label(
                        egui::RichText::new(match kind {
                            JointKind::Corner => "Corner joint",
                            JointKind::Tee => "T joint",
                        })
                        .strong(),
                    );
                    ui.label(egui::RichText::new(format!("{a} · {b}")).small().weak());
                    for &t in JointType::choices(kind) {
                        ui.horizontal(|ui| {
                            let (r, painter) = ui.allocate_painter(
                                Vec2::splat(2.0 * HALF + 2.0),
                                egui::Sense::hover(),
                            );
                            paint_icon(&painter, r.rect.center(), t, false);
                            ui.radio_value(&mut choice, t, choice_label(t, kind, &a, &b));
                        });
                    }
                    ui.horizontal(|ui| {
                        ui.label("Gap");
                        ui.add(ExprValue::new(&mut gap).suffix(" mm"));
                    });
                    if let Some(err) = &error {
                        ui.colored_label(crate::gui::theme::tokens().error, err);
                    } else {
                        ui.label(
                            egui::RichText::new("The view shows the result; OK keeps it.")
                                .small()
                                .weak(),
                        );
                    }
                    ui.horizontal(|ui| {
                        if ui.add(crate::gui::theme::ok_button("OK")).clicked() {
                            ok = true;
                        }
                        if ui.button("Cancel").clicked() {
                            cancel = true;
                        }
                    });
                });
            });
        // A click outside the panel's rectangle (its widgets take their own).
        let inside = ctx
            .input(|i| i.pointer.interact_pos())
            .is_some_and(|p| r.response.rect.contains(p));
        let elsewhere = ctx.input(|i| i.pointer.any_click()) && !inside;
        if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            ok = true;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) || (elsewhere && !fresh) {
            cancel = true;
        }
        let changed = self
            .asm
            .corner_menu
            .as_ref()
            .is_some_and(|e| e.choice != choice || (e.gap - gap).abs() > 1e-12);
        if let Some(e) = self.asm.corner_menu.as_mut() {
            e.choice = choice;
            e.gap = gap;
            e.fresh = false;
        }
        if cancel {
            self.joint_cancel();
        } else if ok {
            self.joint_ok();
        } else if changed {
            self.joint_preview();
        }
    }
}
