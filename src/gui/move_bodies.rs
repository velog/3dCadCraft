//! **Move Bodies** (3D Model › Modify, `docs/spec/2.13-move-bodies.md`): the
//! panel (the Bodies selector, the moves one under another — Free Drag,
//! Move Along Ray, Rotate About Line — and *Click to add*), the preview of
//! the moved bodies' edges, OK / Apply / Cancel and Edit Feature. The
//! geometry is `part::body_op::MoveBodiesOp` + `part/exact.rs::move_bodies`.

use eframe::egui::{self, RichText};

use super::CadCraftApp;
use super::command_ui::{RowEvents, selector_row};
use super::icons::Icon;
use super::panels::{plabel, psection, warning_box};
use super::select::SelItem;
use super::valuefield::ExprValue;
use crate::command::flow::{Flow, PickKind, Selector, colors};
use crate::kernel::Point3;
use crate::part::body_op::{MAX_STEPS, MoveBodiesOp, MoveStep};
use crate::part::{BodyOp, BodyOpFeature, Feature, LineRef};

/// The open Move Bodies command.
#[derive(Clone, Debug)]
pub(super) struct MoveDraft {
    pub def: MoveBodiesOp,
    /// Edit Feature of this feature index.
    pub edit: Option<usize>,
    /// The active selector: `bodies`, or `line` (the direction / axis of
    /// step `step`).
    pub input: &'static str,
    /// The step whose line the `line` selector picks.
    pub step: usize,
}

const BODIES: &[PickKind] = &[PickKind::Body];
const LINES: &[PickKind] = &[PickKind::StraightEdge, PickKind::Axis];

/// A step's line (Move Along Ray's direction, Rotate About Line's axis).
fn line_of(s: &MoveStep) -> Option<&LineRef> {
    match s {
        MoveStep::AlongRay { direction, .. } => Some(direction),
        MoveStep::RotateAboutLine { axis, .. } => Some(axis),
        MoveStep::FreeDrag { .. } => None,
    }
}

fn line_text(l: &LineRef) -> String {
    match l {
        LineRef::Origin(k) => format!("{} Axis", ["X", "Y", "Z"][(*k).min(2)]),
        LineRef::Edge(n) => format!("edge {n}"),
        LineRef::Work(id) => id.clone(),
    }
}

impl MoveDraft {
    pub(super) fn new() -> Self {
        Self {
            def: MoveBodiesOp::new(),
            edit: None,
            input: "bodies",
            step: 0,
        }
    }

    /// The selectors: Bodies, then the line of the step being set.
    pub(super) fn flow(&self) -> Flow {
        let mut v = vec![Selector {
            key: "bodies",
            label: "Bodies",
            prompt: "Select the bodies to move",
            accepts: BODIES,
            single: false,
            required: true,
            count: self.def.bodies.len(),
            color: colors::FACES,
        }];
        let step = self.def.steps.get(self.step);
        if let Some(s) = step.filter(|s| line_of(s).is_some()) {
            let (label, prompt) = match s {
                MoveStep::RotateAboutLine { .. } => {
                    ("Axis", "Select the edge or axis to turn about (or X, Y, Z)")
                }
                _ => (
                    "Direction",
                    "Select the edge or axis to move along (or X, Y, Z)",
                ),
            };
            v.push(Selector {
                key: "line",
                label,
                prompt,
                accepts: LINES,
                single: true,
                required: true,
                count: 1,
                color: colors::PLANE,
            });
        }
        let key = if self.input == "line" && v.len() > 1 {
            "line"
        } else {
            "bodies"
        };
        Flow::new(v, Some(key))
    }

    /// Sets the line of the active step.
    fn set_line(&mut self, l: LineRef) {
        match self.def.steps.get_mut(self.step) {
            Some(MoveStep::AlongRay { direction, .. }) => *direction = l,
            Some(MoveStep::RotateAboutLine { axis, .. }) => *axis = l,
            _ => {}
        }
    }

    /// Body `name` added, or with a second pick removed.
    fn toggle_body(&mut self, name: String) {
        match self.def.bodies.iter().position(|b| *b == name) {
            Some(i) => {
                self.def.bodies.remove(i);
            }
            None => self.def.bodies.push(name),
        }
    }
}

/// A new step of `kind` (0 Free Drag, 1 Move Along Ray, 2 Rotate About
/// Line), keeping `old`'s line and flip when it has them.
fn step_of_kind(kind: usize, old: Option<&MoveStep>) -> MoveStep {
    let line = old.and_then(line_of).cloned().unwrap_or(LineRef::Origin(0));
    let flip = matches!(
        old,
        Some(MoveStep::AlongRay { flip: true, .. } | MoveStep::RotateAboutLine { flip: true, .. })
    );
    match kind {
        1 => MoveStep::AlongRay {
            direction: line,
            flip,
            offset: 0.0,
        },
        2 => MoveStep::RotateAboutLine {
            axis: line,
            flip,
            angle: 0.0,
        },
        _ => MoveStep::FreeDrag {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
    }
}

fn kind_of(s: &MoveStep) -> usize {
    match s {
        MoveStep::FreeDrag { .. } => 0,
        MoveStep::AlongRay { .. } => 1,
        MoveStep::RotateAboutLine { .. } => 2,
    }
}

impl CadCraftApp {
    /// **Move Bodies**: bodies (or faces of bodies) selected before are its
    /// bodies (rule 17).
    pub(super) fn start_move_bodies(&mut self) {
        let pre = self.sel.clone();
        self.finish_sketch();
        self.mode = super::Mode::Normal;
        self.cancel_extrude();
        self.cancel_thicken();
        self.cancel_delete_face();
        self.work_draft = None;
        self.blend_draft = None;
        self.face_sel = None;
        self.edge_sel = None;
        let mut d = MoveDraft::new();
        let body = &self.part().body;
        for x in pre {
            let name = match &x {
                SelItem::Body(n) => Some(n.clone()),
                SelItem::Face(n) => body
                    .find_face(n)
                    .and_then(|f| crate::part::select::body_of_face(body, f)),
                _ => None,
            };
            if let Some(n) = name
                && !d.def.bodies.contains(&n)
            {
                d.def.bodies.push(n);
            }
        }
        self.sel.clear();
        self.move_draft = Some(d);
    }

    /// Edit Feature of Move Bodies feature `k`.
    pub(super) fn move_bodies_edit(&mut self, k: usize) {
        let Some(Feature::BodyOp(BodyOpFeature {
            op: BodyOp::MoveBodies(def),
            ..
        })) = self.part().features.get(k).cloned()
        else {
            return;
        };
        self.roll_back_to(k);
        let mut d = MoveDraft::new();
        d.def = def;
        d.edit = Some(k);
        self.move_draft = Some(d);
    }

    /// A click in the view: a body (Bodies selector) or a straight edge or
    /// work axis (the step's line).
    pub(super) fn move_bodies_click(&mut self, p: egui::Pos2, rect: egui::Rect) {
        let Some(mut d) = self.move_draft.clone() else {
            return;
        };
        if d.flow().current().is_some_and(|s| s.key == "line") {
            let body = &self.part().body;
            let edge = self
                .edge_under(p, rect)
                .filter(|&e| body.edge_direction(e).is_some())
                .and_then(|e| body.edge(e).and_then(|x| x.name.clone()))
                .map(LineRef::Edge);
            let axis = match self.work_axis_or_point_under(p, rect) {
                Some(super::work::WpInput::Line(l)) => Some(l),
                _ => None,
            };
            if let Some(l) = edge.or(axis) {
                d.set_line(l);
            }
        } else {
            let hit = self.camera.ray(p, rect).and_then(|(o, dir)| {
                let dir = dir.try_normalize(1e-12)?;
                self.picker().pick(&o, &dir)
            });
            let body = &self.part().body;
            if let Some(n) = hit.and_then(|(f, _)| crate::part::select::body_of_face(body, f)) {
                d.toggle_body(n);
            }
        }
        self.move_draft = Some(d);
    }

    /// A body row picked in the browser.
    pub(super) fn move_bodies_pick_body(&mut self, name: String) {
        if let Some(d) = self.move_draft.as_mut() {
            d.input = "bodies";
            d.toggle_body(name);
        }
    }

    /// The moved bodies' edges (worker thread), or why it fails.
    pub(super) fn move_bodies_preview(&mut self) -> Result<Vec<Vec<Point3>>, String> {
        let Some(d) = self.move_draft.as_ref() else {
            return Err(String::new());
        };
        if d.def.validate().is_err() {
            return Err(String::new());
        }
        let key = format!("move_bodies|{:?}|{:?}", d.edit, d.def);
        if let Some(r) = self.preview_ready(&key) {
            return r;
        }
        let mut trial = self.part().clone();
        let (def, edit) = (d.def.clone(), d.edit);
        self.preview_async(key, move || {
            let names = def.bodies.clone();
            let f = BodyOpFeature::new(BodyOp::MoveBodies(def));
            let built = match edit {
                Some(k) => trial.replace_feature(k, Feature::BodyOp(f)),
                None => trial.add_body_op(f),
            };
            built
                .map(|_| {
                    let faces: std::collections::HashSet<String> = names
                        .iter()
                        .flat_map(|b| crate::part::select::faces_of_body(&trial.body, b))
                        .filter_map(|f| trial.body.face(f).and_then(|f| f.name.clone()))
                        .collect();
                    super::combine_split::face_edges(&trial, |n| faces.contains(n))
                })
                .map_err(|e| e.to_string())
        })
    }

    /// OK (`keep_open` false) or Apply.
    pub(super) fn commit_move_bodies(&mut self, keep_open: bool) {
        let Some(d) = self.move_draft.clone() else {
            return;
        };
        let cmd = if d.edit.is_some() {
            "Edit Feature"
        } else {
            "Move Bodies"
        };
        self.checkpoint(cmd);
        let f = BodyOpFeature::new(BodyOp::MoveBodies(d.def.clone()));
        let built = match d.edit {
            Some(k) => self.part_mut().replace_feature(k, Feature::BodyOp(f)),
            None => self.part_mut().add_body_op(f),
        };
        if built.is_err()
            && let Some(k) = d.edit
        {
            self.part_mut().rollback = Some(k);
            let _ = self.part_mut().regenerate();
        }
        match built {
            Ok(_) => {
                let k = d.edit.unwrap_or(self.part().last_inserted());
                self.file_status = None;
                self.scene_dirty = true;
                if keep_open && d.edit.is_none() {
                    self.move_draft = Some(MoveDraft::new());
                } else {
                    self.move_draft = None;
                    self.end_feature_edit();
                    self.selection = super::Selection::Feature(k);
                }
            }
            Err(e) => {
                self.history.cancel();
                self.file_status = Some(Err(format!("{cmd}: {e}")));
            }
        }
    }

    /// Closes the command (Cancel / Esc).
    pub(super) fn cancel_move_bodies(&mut self) {
        if self.move_draft.take().is_some() {
            self.end_feature_edit();
        }
    }

    /// Makes selector `key` active.
    pub(super) fn move_bodies_activate(&mut self, key: &str) {
        if let Some(d) = self.move_draft.as_mut() {
            d.input = if key == "line" { "line" } else { "bodies" };
        }
    }

    /// The status-bar prompt.
    pub(super) fn move_bodies_prompt(&self) -> Option<String> {
        let d = self.move_draft.as_ref()?;
        let f = d.flow();
        let p = f.current().map_or("", |s| s.prompt);
        let ok = if d.def.validate().is_ok() {
            ", or OK (Enter)"
        } else {
            ""
        };
        Some(format!("Move Bodies: {p}{ok}"))
    }

    /// The panel (Properties).
    pub(super) fn move_bodies_panel(&mut self, ui: &mut egui::Ui) {
        let Some(mut d) = self.move_draft.clone() else {
            return;
        };
        self.panel_header(ui, Icon::MoveBodies, "Move Bodies");
        let mut ev = RowEvents::default();
        psection(ui, "move_bodies", "Bodies", |ui| {
            let flow = d.flow();
            if let Some(s) = flow.selectors.first() {
                let text = match d.def.bodies.as_slice() {
                    [] => String::new(),
                    [one] => one.clone(),
                    many => format!("{} selected", many.len()),
                };
                selector_row(ui, s, flow.active == Some(0), &text, &mut ev);
                ui.end_row();
            }
        });
        let mut remove: Option<usize> = None;
        let n = d.def.steps.len();
        for k in 0..n {
            let title = format!("Move {}", k + 1);
            psection(ui, &format!("move_step_{k}"), &title, |ui| {
                plabel(ui, "Move Type");
                ui.horizontal(|ui| {
                    let mut kind = kind_of(&d.def.steps[k]);
                    let was = kind;
                    egui::ComboBox::from_id_salt(("move_type", k))
                        .selected_text(d.def.steps[k].label())
                        .show_ui(ui, |ui| {
                            for (i, l) in ["Free Drag", "Move Along Ray", "Rotate About Line"]
                                .iter()
                                .enumerate()
                            {
                                ui.selectable_value(&mut kind, i, *l);
                            }
                        });
                    if kind != was {
                        d.def.steps[k] = step_of_kind(kind, d.def.steps.get(k));
                        d.step = k;
                        d.input = if kind == 0 { "bodies" } else { "line" };
                    }
                    if n > 1
                        && ui
                            .small_button("x")
                            .on_hover_text("Remove this move")
                            .clicked()
                    {
                        remove = Some(k);
                    }
                });
                ui.end_row();
                let active_line = d.input == "line" && d.step == k;
                match &mut d.def.steps[k] {
                    MoveStep::FreeDrag { x, y, z } => {
                        for (l, v) in [("X", x), ("Y", y), ("Z", z)] {
                            plabel(ui, l);
                            ui.add(ExprValue::new(v).suffix(" mm"));
                            ui.end_row();
                        }
                    }
                    MoveStep::AlongRay {
                        direction: line,
                        flip,
                        offset: value,
                    }
                    | MoveStep::RotateAboutLine {
                        axis: line,
                        flip,
                        angle: value,
                    } => {
                        let _ = value;
                        plabel(ui, "");
                        ui.horizontal(|ui| {
                            let r = ui.selectable_label(active_line, line_text(line));
                            if r.clicked() {
                                d.step = k;
                                d.input = "line";
                            }
                            for (i, a) in ["X", "Y", "Z"].iter().enumerate() {
                                if ui
                                    .small_button(*a)
                                    .on_hover_text(format!("The {a} axis of the part"))
                                    .clicked()
                                {
                                    *line = LineRef::Origin(i);
                                }
                            }
                            ui.checkbox(flip, "Flip")
                                .on_hover_text("The other way along the line");
                        });
                        ui.end_row();
                    }
                }
                match &mut d.def.steps[k] {
                    MoveStep::AlongRay { offset, .. } => {
                        plabel(ui, "Offset");
                        ui.add(ExprValue::new(offset).suffix(" mm"));
                        ui.end_row();
                    }
                    MoveStep::RotateAboutLine { angle, .. } => {
                        plabel(ui, "Angle");
                        let mut deg = angle.to_degrees();
                        ui.add(ExprValue::new(&mut deg).suffix("°"));
                        *angle = deg.to_radians();
                        ui.end_row();
                    }
                    MoveStep::FreeDrag { .. } => {}
                }
            });
        }
        if let Some(k) = remove {
            d.def.steps.remove(k);
            d.step = d.step.min(d.def.steps.len().saturating_sub(1));
            d.input = "bodies";
        }
        ui.add_enabled_ui(d.def.steps.len() < MAX_STEPS, |ui| {
            if ui
                .button("Click to add")
                .on_hover_text("Another move (Free Drag, Move Along Ray or Rotate About Line)")
                .clicked()
            {
                d.def.steps.push(step_of_kind(0, None));
            }
        });
        if let Some(k) = ev.activate {
            d.input = if k == "line" { "line" } else { "bodies" };
        }
        if ev.clear.is_some() {
            d.def.bodies.clear();
        }
        let editing = d.edit.is_some();
        self.move_draft = Some(d.clone());
        let ready = d.def.validate().is_ok();
        let preview = self.move_bodies_preview();
        match &preview {
            Err(e) if e == super::PREVIEW_PENDING => {
                ui.label(e);
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(50));
            }
            Err(e) if ready && !e.is_empty() => warning_box(ui, e),
            Err(_) if !ready => {
                if let Err(e) = d.def.validate() {
                    ui.label(RichText::new(e).weak());
                }
            }
            _ => {}
        }
        let ok =
            ready && (preview.is_ok() || matches!(&preview, Err(e) if e == super::PREVIEW_PENDING));
        ui.horizontal(|ui| {
            if ui
                .add_enabled(ok, egui::Button::new("OK").min_size(egui::vec2(60.0, 22.0)))
                .on_hover_text("Enter, Ctrl+Enter")
                .clicked()
                || super::keys::panel_ok_key(ui, ok)
            {
                self.commit_move_bodies(false);
            }
            if !editing
                && ui
                    .add_enabled(
                        ok,
                        egui::Button::new("Apply").min_size(egui::vec2(60.0, 22.0)),
                    )
                    .on_hover_text("Create it and keep the command open")
                    .clicked()
            {
                self.commit_move_bodies(true);
            }
            if ui
                .add(egui::Button::new("Cancel").min_size(egui::vec2(60.0, 22.0)))
                .clicked()
            {
                self.cancel_move_bodies();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_kind_keeps_the_line_and_the_flip() {
        let old = MoveStep::AlongRay {
            direction: LineRef::Edge("Box1:E1".into()),
            flip: true,
            offset: 5.0,
        };
        match step_of_kind(2, Some(&old)) {
            MoveStep::RotateAboutLine { axis, flip, angle } => {
                assert_eq!(axis, LineRef::Edge("Box1:E1".into()));
                assert!(flip);
                assert_eq!(angle, 0.0);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(kind_of(&step_of_kind(0, Some(&old))), 0);
    }

    #[test]
    fn the_line_selector_follows_the_step() {
        let mut d = MoveDraft::new();
        assert_eq!(d.flow().selectors.len(), 1, "Free Drag has no line");
        d.def.steps[0] = step_of_kind(1, None);
        d.input = "line";
        let f = d.flow();
        assert_eq!(f.selectors.len(), 2);
        assert_eq!(f.current().map(|s| s.key), Some("line"));
        d.set_line(LineRef::Origin(2));
        assert_eq!(line_of(&d.def.steps[0]), Some(&LineRef::Origin(2)));
        d.toggle_body("Extrusion1".into());
        d.toggle_body("Extrusion2".into());
        d.toggle_body("Extrusion1".into());
        assert_eq!(d.def.bodies, ["Extrusion2"]);
    }
}
