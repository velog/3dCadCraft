//! **Next** and **Back** in every command (`docs/spec/0-ui.md` §4.1, owner's
//! rule 21): the right-click menu moves to the next input without knowing
//! which command is open. A command's inputs are its selectors ([`Flow`]; the
//! ones of `command_ui` as they are, the others described here) and then the
//! values of its panel, edited next to the cursor (`cursor_value`).
//!
//! Sketch Mirror: select the lines, right click, Next, pick the mirror line.
//! Extrude: pick the profile, Next, type the distance at the cursor.

use eframe::egui;

use super::CadCraftApp;
use super::cursor_value;
use super::icons::{Icon, menu_item};
use super::sketch_tools::Tool;
use crate::command::flow::{Flow, PickKind as K, Selector, colors};

const SLOTS: [u8; 3] = [40, 120, 230];

/// A selector of a command outside the shared flow.
fn input(
    key: &'static str,
    label: &'static str,
    accepts: &'static [K],
    single: bool,
    required: bool,
    count: usize,
    color: [u8; 3],
) -> Selector {
    Selector {
        key,
        label,
        prompt: label,
        accepts,
        single,
        required,
        count,
        color,
    }
}

/// The input a sketch's Move, Copy, Rotate, Mirror, Scale or Stretch takes
/// after its geometry.
fn transform_target(t: Tool) -> Option<(&'static str, &'static [K])> {
    match t {
        Tool::Mirror => Some(("Mirror Line", &[K::SketchCurve])),
        Tool::Rotate => Some(("Center Point", &[K::SketchPoint])),
        Tool::Move | Tool::Copy | Tool::Scale | Tool::Stretch => {
            Some(("Base Point", &[K::SketchPoint]))
        }
        _ => None,
    }
}

impl CadCraftApp {
    /// The inputs of whatever command is open.
    pub(super) fn step_flow(&self) -> Option<Flow> {
        if let Some(f) = self.command_flow() {
            return Some(f);
        }
        if self.editing.is_some() {
            return self.sketch_steps();
        }
        if let Some(f) = self.constrain_flow() {
            return Some(f);
        }
        if let Some(d) = &self.extrude {
            let mut v = vec![input(
                "profiles",
                "Profiles",
                &[K::Region],
                false,
                true,
                d.def.profiles().len(),
                SLOTS,
            )];
            if matches!(d.def, super::DraftDef::Revolve(_)) {
                v.push(input(
                    "axis",
                    "Axis",
                    &[K::StraightEdge, K::Axis, K::SketchCurve],
                    true,
                    true,
                    usize::from(d.axis_set),
                    colors::DIRECTION,
                ));
            }
            let active = if d.axis_pick { "axis" } else { "profiles" };
            return Some(Flow::new(v, Some(active)));
        }
        if let Some(d) = &self.blend_draft {
            let (label, kinds): (_, &[K]) = if d.picks_faces() {
                ("Faces", &[K::Face])
            } else {
                ("Edges", &[K::Edge])
            };
            let n = d.def.edges.len() + d.def.sets.iter().map(|s| s.edges.len()).sum::<usize>();
            let v = vec![input(
                "edges",
                label,
                kinds,
                false,
                !d.picks_faces(),
                n,
                colors::EDGES,
            )];
            return Some(Flow::new(v, Some("edges")));
        }
        if let Some(d) = &self.sweep_draft {
            use super::sweep::SweepInput as S;
            let mut v = vec![
                input(
                    "profiles",
                    "Profiles",
                    &[K::Region],
                    false,
                    true,
                    d.def.profiles.len(),
                    SLOTS,
                ),
                input(
                    "path",
                    "Path",
                    &[K::SketchCurve, K::Edge],
                    false,
                    true,
                    usize::from(!d.def.path.is_empty()),
                    colors::DIRECTION,
                ),
            ];
            if d.def.sweep_type == crate::part::SweepType::PathAndGuideRail {
                v.push(input(
                    "rail",
                    "Guide Rail",
                    &[K::SketchCurve, K::Edge],
                    false,
                    true,
                    usize::from(d.def.rail.is_some()),
                    colors::GUIDES,
                ));
            }
            let active = match d.input {
                S::Profiles => "profiles",
                S::Path => "path",
                S::Rail => "rail",
            };
            return Some(Flow::new(v, Some(active)));
        }
        if let Some(d) = &self.coil_draft {
            return Some(super::coil_rib::coil_flow(d));
        }
        if let Some(d) = &self.rib_draft {
            return Some(super::coil_rib::rib_flow(d));
        }
        if let Some(d) = &self.pattern_draft {
            use super::pattern_feature::PatternInput as P;
            use crate::part::PatternKind;
            const LINES: &[K] = &[K::StraightEdge, K::Axis, K::PlanarFace, K::Plane];
            let geometry = if d.def.solid_mode {
                d.def.solids.len()
            } else {
                d.def.features.len()
            };
            let mut v = vec![input(
                "geometry",
                if d.def.solid_mode {
                    "Solids"
                } else {
                    "Features"
                },
                &[K::Face, K::Body],
                false,
                !d.def.solid_mode,
                geometry,
                SLOTS,
            )];
            // What a placement selector holds is read back from its own
            // panel row; here only whether the command can go on matters.
            match &d.def.kind {
                PatternKind::Rectangular { .. } => {
                    v.push(input(
                        "dir1",
                        "Direction 1",
                        LINES,
                        true,
                        false,
                        0,
                        colors::DIRECTION,
                    ));
                    v.push(input(
                        "dir2",
                        "Direction 2",
                        LINES,
                        true,
                        false,
                        0,
                        colors::GUIDES,
                    ));
                }
                PatternKind::Circular { .. } => {
                    v.push(input(
                        "axis",
                        "Rotation Axis",
                        LINES,
                        true,
                        false,
                        0,
                        colors::DIRECTION,
                    ));
                }
                PatternKind::Mirror { .. } => {
                    v.push(input(
                        "plane",
                        "Mirror Plane",
                        &[K::PlanarFace, K::Plane],
                        true,
                        false,
                        0,
                        colors::PLANE,
                    ));
                }
                PatternKind::Sketch { .. } => {
                    v.push(input(
                        "sketch",
                        "Sketch",
                        &[K::Sketch],
                        true,
                        false,
                        0,
                        colors::DIRECTION,
                    ));
                    v.push(input(
                        "base",
                        "Base Point",
                        &[K::Point],
                        true,
                        false,
                        0,
                        colors::GUIDES,
                    ));
                }
            }
            let active = match d.input {
                P::Geometry => "geometry",
                P::Dir1 => "dir1",
                P::Dir2 => "dir2",
                P::Axis => "axis",
                P::Plane => "plane",
                P::Sketch => "sketch",
                P::Base => "base",
            };
            return Some(Flow::new(v, Some(active)));
        }
        if let Some(d) = &self.hole_draft {
            let v = vec![input(
                "centers",
                "Positions",
                &[K::SketchPoint, K::Edge, K::PlanarFace, K::Face],
                false,
                d.edit.is_none(),
                d.count(),
                SLOTS,
            )];
            return Some(Flow::new(v, Some("centers")));
        }
        if let Some(d) = &self.thread_draft {
            let v = vec![input(
                "face",
                "Face",
                &[K::Face],
                true,
                true,
                usize::from(d.face.is_some()),
                colors::FACES,
            )];
            return Some(Flow::new(v, Some("face")));
        }
        if let Some(d) = &self.work_draft {
            let v = vec![input(
                "inputs",
                "Inputs",
                &[
                    K::Point,
                    K::Axis,
                    K::Edge,
                    K::Face,
                    K::Plane,
                    K::SketchPoint,
                    K::SketchCurve,
                ],
                false,
                false,
                d.picks.len(),
                SLOTS,
            )];
            return Some(Flow::new(v, Some("inputs")));
        }
        None
    }

    /// The inputs of a sketch command that takes geometry first.
    fn sketch_steps(&self) -> Option<Flow> {
        use super::pattern_dialog::Slot;
        let t = &self.tools;
        let geometry = t.selected.len();
        if let Some(d) = &t.pattern_dialog {
            let v = super::pattern_dialog::PatternDialog::slots(d.tool)
                .iter()
                .map(|&s| {
                    let (key, kinds): (&'static str, &'static [K]) = match s {
                        Slot::Geometry => ("geometry", &[K::SketchCurve]),
                        Slot::Dir1 => ("dir1", &[K::SketchCurve]),
                        Slot::Dir2 => ("dir2", &[K::SketchCurve]),
                        Slot::Center => ("center", &[K::SketchPoint]),
                        Slot::Path => ("path", &[K::SketchCurve]),
                        Slot::Base => ("base", &[K::SketchPoint]),
                        Slot::Targets => ("targets", &[K::SketchPoint]),
                    };
                    let count = match s {
                        Slot::Geometry => geometry,
                        Slot::Dir1 => usize::from(d.dir[0].is_some()),
                        Slot::Dir2 => usize::from(d.dir[1].is_some()),
                        Slot::Center => usize::from(d.center.is_some()),
                        Slot::Path => usize::from(d.path.is_some()),
                        Slot::Base => usize::from(d.base.is_some()),
                        Slot::Targets => d.targets.len(),
                    };
                    input(
                        key,
                        s.label(),
                        kinds,
                        false,
                        s == Slot::Geometry,
                        count,
                        SLOTS,
                    )
                })
                .collect();
            let active = match d.active {
                Slot::Geometry => "geometry",
                Slot::Dir1 => "dir1",
                Slot::Dir2 => "dir2",
                Slot::Center => "center",
                Slot::Path => "path",
                Slot::Base => "base",
                Slot::Targets => "targets",
            };
            return Some(Flow::new(v, Some(active)));
        }
        let (label, kinds) = transform_target(t.tool)?;
        let v = vec![
            input(
                "geometry",
                "Geometry",
                &[K::SketchCurve],
                false,
                true,
                geometry,
                SLOTS,
            ),
            input(
                "target",
                label,
                kinds,
                true,
                false,
                t.pick_count(),
                colors::DIRECTION,
            ),
        ];
        let active = if t.transform_ready() {
            "target"
        } else {
            "geometry"
        };
        Some(Flow::new(v, Some(active)))
    }

    /// Makes input `key` of the open command active.
    fn step_activate(&mut self, key: &str) {
        if self.command_flow().is_some() {
            self.command_activate(key);
            return;
        }
        if self.editing.is_some() {
            self.sketch_step_activate(key);
            return;
        }
        if self.constrain_activate(key) {
            return;
        }
        if let Some(d) = self.extrude.as_mut() {
            d.axis_pick = key == "axis";
        } else if let Some(d) = self.sweep_draft.as_mut() {
            use super::sweep::SweepInput as S;
            d.input = match key {
                "path" => S::Path,
                "rail" => S::Rail,
                _ => S::Profiles,
            };
        } else if let Some(d) = self.coil_draft.as_mut() {
            d.active = if key == "axis" { "axis" } else { "profiles" };
        } else if let Some(d) = self.rib_draft.as_mut() {
            d.active = if key == "centers" {
                "centers"
            } else {
                "profile"
            };
        } else if let Some(d) = self.pattern_draft.as_mut() {
            use super::pattern_feature::PatternInput as P;
            d.input = match key {
                "dir1" => P::Dir1,
                "dir2" => P::Dir2,
                "axis" => P::Axis,
                "plane" => P::Plane,
                "sketch" => P::Sketch,
                "base" => P::Base,
                _ => P::Geometry,
            };
        }
    }

    fn sketch_step_activate(&mut self, key: &str) {
        use super::pattern_dialog::Slot;
        if let Some(d) = self.tools.pattern_dialog.as_mut() {
            d.active = match key {
                "dir1" => Slot::Dir1,
                "dir2" => Slot::Dir2,
                "center" => Slot::Center,
                "path" => Slot::Path,
                "base" => Slot::Base,
                "targets" => Slot::Targets,
                _ => Slot::Geometry,
            };
            return;
        }
        if key == "geometry" {
            self.tools.reselect_geometry();
        } else {
            // Moves on when geometry is selected, says so when not.
            self.tool_op(|t, ps| t.finish(ps));
        }
    }

    /// Whether Next has somewhere to go.
    pub(super) fn step_can_next(&self, ctx: &egui::Context) -> bool {
        if let Some(k) = self.cv.step {
            return cursor_value::stepped(k, cursor_value::fields(ctx).len(), 1).is_some();
        }
        let more = self.step_flow().is_some_and(|f| {
            f.menu(false, false)
                .iter()
                .any(|(i, on)| *i == crate::command::flow::MenuItem::Continue && *on)
        });
        more || self.cursor_value_on(ctx)
    }

    /// Whether Back has somewhere to go.
    pub(super) fn step_can_back(&self) -> bool {
        self.cv.step.is_some()
            || self.step_flow().is_some_and(|f| {
                f.menu(false, false)
                    .iter()
                    .any(|(i, on)| *i == crate::command::flow::MenuItem::Back && *on)
            })
    }

    /// **Next**: the next input; after the last selector, the panel's values
    /// one by one in the editor next to the cursor.
    pub(super) fn step_next(&mut self, ctx: &egui::Context) {
        if let Some(k) = self.cv.step {
            if let Some(to) = cursor_value::stepped(k, cursor_value::fields(ctx).len(), 1) {
                self.cursor_value_open(ctx, to, None);
            }
            return;
        }
        if let Some(mut f) = self.step_flow()
            && f.continue_()
            && let Some(key) = f.active_key()
        {
            self.step_activate(key);
            return;
        }
        if self.cursor_value_on(ctx) {
            self.cursor_value_open(ctx, 0, None);
        }
    }

    /// **Back**: the previous input (from the first value, the last
    /// selector).
    pub(super) fn step_back(&mut self, ctx: &egui::Context) {
        if let Some(k) = self.cv.step {
            match cursor_value::stepped(k, cursor_value::fields(ctx).len(), -1) {
                Some(to) => self.cursor_value_open(ctx, to, None),
                None => self.cursor_value_close(),
            }
            return;
        }
        if let Some(mut f) = self.step_flow()
            && f.back()
            && let Some(key) = f.active_key()
        {
            self.step_activate(key);
        }
    }

    /// Next and Back at the top of a command's right-click menu. Returns
    /// whether the open command has inputs to step through.
    pub(super) fn step_menu(&mut self, ui: &mut egui::Ui) -> bool {
        if self.step_flow().is_none() {
            return false;
        }
        let ctx = ui.ctx().clone();
        if menu_item(ui, Some(Icon::StepNext), "Next", self.step_can_next(&ctx))
            .on_hover_text("The next input of the command")
            .clicked()
        {
            self.step_next(&ctx);
            ui.close();
        }
        if menu_item(ui, Some(Icon::StepBack), "Back", self.step_can_back())
            .on_hover_text("The previous input of the command")
            .clicked()
        {
            self.step_back(&ctx);
            ui.close();
        }
        true
    }

    /// OK and Cancel of a panel in the right-click menu: the same as the
    /// keys, so every panel answers them already.
    pub(super) fn ok_cancel_menu(&mut self, ui: &mut egui::Ui) {
        let ready = self.step_flow().is_none_or(|f| f.ready());
        if menu_item(ui, Some(Icon::MenuOk), "OK (Ctrl+Enter)", ready).clicked() {
            self.inject_keys
                .push((egui::Key::Enter, egui::Modifiers::COMMAND));
            ui.close();
        }
        if menu_item(ui, Some(Icon::MenuCancel), "Cancel (Esc)", true).clicked() {
            self.inject_keys
                .push((egui::Key::Escape, egui::Modifiers::NONE));
            ui.close();
        }
    }

    /// The command menu's entries for an open Fillet / Chamfer: how a click
    /// takes edges (Select Mode of its panel).
    pub(super) fn blend_mode_menu(&mut self, ui: &mut egui::Ui) {
        use super::blend::EdgeMode as M;
        let Some(d) = self.blend_draft.as_mut() else {
            return;
        };
        ui.separator();
        for (m, label) in [
            (M::Edge, "Edge"),
            (M::Loop, "Select Loop"),
            (M::Feature, "Select Feature"),
        ] {
            if ui.radio(d.mode == m, label).clicked() {
                d.mode = m;
                ui.close();
            }
        }
    }

    /// Whether the open command has a direction to flip.
    pub(super) fn flip_available(&self) -> bool {
        self.editing.is_none()
            && (self.extrude.is_some()
                || self.coil_draft.is_some()
                || self.rib_draft.is_some()
                || self.thicken_draft.is_some())
    }

    /// Flip Direction of the open command.
    pub(super) fn flip_direction(&mut self) {
        use crate::part::ExtentDirection as E;
        let turn = |x: &mut E| {
            *x = match *x {
                E::Default => E::Flipped,
                E::Flipped => E::Default,
                other => other,
            };
        };
        if let Some(d) = self.extrude.as_mut() {
            match &mut d.def {
                super::DraftDef::Extrude(e) => turn(&mut e.direction),
                super::DraftDef::Revolve(r) => turn(&mut r.direction),
            }
            d.auto_flipped = false;
        }
        if let Some(d) = self.coil_draft.as_mut() {
            d.def.flip = !d.def.flip;
        }
        if let Some(d) = self.rib_draft.as_mut() {
            d.def.flip = !d.def.flip;
        }
        if let Some(d) = self.thicken_draft.as_mut() {
            use crate::part::body_op::ThickenDirection as T;
            d.def.direction = match d.def.direction {
                T::Direction1 => T::Direction2,
                T::Direction2 => T::Direction1,
                other => other,
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The commands of `keys::open_state` with inputs to step through.
    const STEPPED: &[&str] = &[
        "Sheet Metal",
        "Work Feature",
        "Fillet / Chamfer",
        "Shell",
        "Sweep",
        "Loft",
        "Coil",
        "Rib",
        "Face Draft",
        "Combine",
        "Split",
        "Planned Command",
        "Thicken/Offset",
        "Delete Face",
        "Move Bodies",
        "Pattern / Mirror",
        "Hole",
        "Thread",
        "Sketch Dialog",
        "Constrain",
    ];
    /// Those without: one pick and they are done, or not a command with inputs.
    const NO_STEPS: &[&str] = &[
        // One place clicked, then a panel of values: nothing to step through.
        "Text",
        "Drawing View",
        "Pick Plane",
        "Section View",
        "Pick Target",
        "Project Geometry",
        "Replace Reference",
        "Delete Features",
        "Rename",
        "Edit Form",
    ];

    /// Rule 21: a new command in `open_state` has to say which it is, and
    /// then belongs in `step_flow`.
    #[test]
    fn every_open_command_is_classified_for_next() {
        let src = include_str!("keys.rs");
        let list = src
            .split("let open: [")
            .nth(1)
            .and_then(|s| s.split("];").next())
            .unwrap_or_default();
        let names: Vec<&str> = list.split('"').skip(1).step_by(2).collect();
        assert!(names.len() > 20, "{names:?}");
        for n in names {
            assert!(
                STEPPED.contains(&n) || NO_STEPS.contains(&n),
                "{n}: add it to step_flow (rule 21) and to STEPPED, or give it to NO_STEPS"
            );
        }
    }

    /// Mirror's second input is the line, the point tools' a point.
    #[test]
    fn a_transform_goes_on_to_its_target() {
        assert_eq!(
            transform_target(Tool::Mirror).map(|t| t.0),
            Some("Mirror Line")
        );
        assert_eq!(
            transform_target(Tool::Rotate).map(|t| t.0),
            Some("Center Point")
        );
        assert_eq!(
            transform_target(Tool::Move).map(|t| t.0),
            Some("Base Point")
        );
        assert!(transform_target(Tool::Line).is_none());
    }
}
