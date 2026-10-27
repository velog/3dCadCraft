//! Keyboard rules shared by every command (`docs/spec/0.15-context-menus-shortcuts.md`
//! §4.4, §3.4): which command is open ([`OpenState`], one central check
//! instead of a test per draft), when Enter / Space / the menu may repeat the
//! last command, single-letter aliases only without Ctrl / Alt, and Enter /
//! Ctrl+Enter in the feature panels ([`PanelKey`]).
//!
//! The decisions are plain functions on plain data, so they are tested
//! without a window; `CadCraftApp::open_state` fills the data from the app.

use eframe::egui;

use super::CadCraftApp;

/// What is open right now, read from the app once per decision.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct OpenState {
    /// The open command with a panel or a pick step (Inventor's name).
    pub command: Option<&'static str>,
    /// It uses the shared selector flow (`command_ui`: Enter = Continue/OK
    /// is handled there).
    pub flow: bool,
    /// A sketch tool other than Select is active.
    pub sketch_tool: bool,
    /// Curves are selected in the edited sketch (Enter belongs to them).
    pub sketch_selection: bool,
}

impl OpenState {
    /// Whether any command is open (a feature panel, a pick step, a sketch
    /// tool or dialog).
    pub(super) fn command_open(&self) -> bool {
        self.command.is_some() || self.sketch_tool
    }

    /// Enter / Space repeat the last command: nothing open, nothing
    /// selected in the sketch.
    pub(super) fn repeat_on_key(&self) -> bool {
        !self.command_open() && !self.sketch_selection
    }

    /// *Repeat <command>* is offered in the view's right-click menu.
    pub(super) fn repeat_in_menu(&self) -> bool {
        !self.command_open()
    }
}

/// Single-letter aliases (S, E, R, H, L, C, D, X, O): never with Ctrl or
/// Alt (Alt+S is the command search, Ctrl+S Save); Shift+letter is the same
/// alias (Inventor, spec §5.1 rule 5).
pub(super) fn letter_alias(m: egui::Modifiers) -> bool {
    !(m.alt || m.ctrl || m.command || m.mac_cmd)
}

/// A key for the open feature panel, decided once a frame in `shortcuts`
/// and read by the panel's OK button ([`panel_ok_key`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PanelKey {
    /// Ctrl+Enter: OK when the command can finish.
    Ok,
    /// Enter: OK when every required input is there, else Continue.
    Enter,
}

/// The panel key of this frame: `enter` / `ctrl` pressed with no text field
/// focused. Commands with the shared flow get Enter through `command_ui`;
/// in a sketch, plain Enter finishes the tool (`SketchTools::finish`).
pub(super) fn panel_key(s: &OpenState, enter: bool, ctrl: bool) -> Option<PanelKey> {
    if !enter || !s.command_open() {
        return None;
    }
    if s.flow {
        return None;
    }
    if ctrl {
        return Some(PanelKey::Ok);
    }
    if s.sketch_tool {
        return None;
    }
    s.command.map(|_| PanelKey::Enter)
}

/// What a panel does with its key: OK when `ready`; Enter on an
/// incomplete command is Continue (the panel's own step, if it has one).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PanelAnswer {
    /// Finish the command.
    Ok,
    /// Go on to the next input (or stay on the current one).
    Continue,
}

/// The answer of a panel whose OK is enabled when `ready`.
pub(super) fn panel_answer(key: Option<PanelKey>, ready: bool) -> Option<PanelAnswer> {
    match key? {
        _ if ready => Some(PanelAnswer::Ok),
        PanelKey::Ok => None,
        PanelKey::Enter => Some(PanelAnswer::Continue),
    }
}

fn key_id() -> egui::Id {
    egui::Id::new("cadcraft_panel_key")
}

/// Stores this frame's panel key (`None` clears last frame's).
pub(super) fn set_panel_key(ctx: &egui::Context, key: Option<PanelKey>) {
    ctx.data_mut(|d| d.insert_temp(key_id(), key));
}

/// This frame's panel key, taken (one panel answers it). Ctrl+Enter typed
/// inside a field of the panel also counts (`shortcuts` skips the keys then).
pub(super) fn take_panel_key(ui: &egui::Ui) -> Option<PanelKey> {
    let typed_ctrl_enter = ui.ctx().egui_wants_keyboard_input()
        && ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter));
    let key = ui
        .ctx()
        .data_mut(|d| d.remove_temp::<Option<PanelKey>>(key_id()).flatten());
    key.or(typed_ctrl_enter.then_some(PanelKey::Ok))
}

/// Whether the panel's OK (enabled when `ready`) is triggered by a key.
pub(super) fn panel_ok_key(ui: &egui::Ui, ready: bool) -> bool {
    panel_answer(take_panel_key(ui), ready) == Some(PanelAnswer::Ok)
}

impl CadCraftApp {
    /// What is open now: the one place that lists every command.
    pub(super) fn open_state(&self) -> OpenState {
        let extrude = self.extrude.as_ref().map(|d| d.def.command());
        let prim = self.prim_draft.as_ref().map(|d| d.kind.label());
        let sketch_dialog = self.editing.is_some()
            && (self.tools.pattern_dialog.is_some()
                || self.tools.has_dialog()
                || self.tools.edit_pattern.is_some());
        let open: [(bool, &'static str); 26] = [
            (self.drw_open(), "Drawing View"),
            (matches!(self.mode, super::Mode::PickPlane(_)), "Pick Plane"),
            (self.sm.draft.is_some(), "Sheet Metal"),
            (self.work_draft.is_some(), "Work Feature"),
            (self.blend_draft.is_some(), "Fillet / Chamfer"),
            (self.shell_draft.is_some(), "Shell"),
            (self.sweep_draft.is_some(), "Sweep"),
            (self.loft_draft.is_some(), "Loft"),
            (self.coil_draft.is_some(), "Coil"),
            (self.rib_draft.is_some(), "Rib"),
            (self.draft_draft.is_some(), "Face Draft"),
            (self.combine_draft.is_some(), "Combine"),
            (self.split_draft.is_some(), "Split"),
            (self.part_planned.is_some(), "Planned Command"),
            (self.thicken_draft.is_some(), "Thicken/Offset"),
            (self.delete_draft.is_some(), "Delete Face"),
            (
                self.section.as_ref().is_some_and(|s| s.editing),
                "Section View",
            ),
            (self.pattern_draft.is_some(), "Pattern / Mirror"),
            (self.hole_draft.is_some(), "Hole"),
            (self.thread_draft.is_some(), "Thread"),
            (self.target_pick.is_some(), "Pick Target"),
            (sketch_dialog, "Sketch Dialog"),
            (
                self.editing.is_some() && self.project_geometry,
                "Project Geometry",
            ),
            (self.relink.is_some(), "Replace Reference"),
            (self.confirm_delete.is_some(), "Delete Features"),
            (self.rename.is_some(), "Rename"),
        ];
        let command = extrude
            .or(prim)
            .or_else(|| open.iter().find(|(on, _)| *on).map(|(_, n)| *n));
        OpenState {
            command,
            flow: self.flow_command_open(),
            sketch_tool: self.editing.is_some()
                && self.tools.tool != super::sketch_tools::Tool::Select,
            sketch_selection: self.editing.is_some() && !self.tools.selected.is_empty(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(name: &'static str) -> OpenState {
        OpenState {
            command: Some(name),
            ..Default::default()
        }
    }

    /// Every command with a panel blocks Repeat on Enter / Space and in the
    /// menu (spec 0.15 §3.1.1, §4.4 — the A0 bug: Fillet, Hole… repeated).
    #[test]
    fn no_repeat_while_a_command_is_open() {
        for name in [
            "Extrude",
            "Revolve",
            "Box",
            "Fillet / Chamfer",
            "Hole",
            "Thread",
            "Work Feature",
            "Pattern / Mirror",
            "Sweep",
            "Loft",
            "Coil",
            "Rib",
            "Combine",
            "Split",
            "Shell",
            "Face Draft",
            "Pick Plane",
        ] {
            let s = open(name);
            assert!(s.command_open(), "{name}");
            assert!(!s.repeat_on_key(), "{name}");
            assert!(!s.repeat_in_menu(), "{name}");
        }
        let tool = OpenState {
            sketch_tool: true,
            ..Default::default()
        };
        assert!(!tool.repeat_on_key() && !tool.repeat_in_menu());
        let idle = OpenState::default();
        assert!(idle.repeat_on_key() && idle.repeat_in_menu());
        // Curves selected in a sketch: Enter is theirs, the menu still repeats.
        let selected = OpenState {
            sketch_selection: true,
            ..Default::default()
        };
        assert!(!selected.repeat_on_key() && selected.repeat_in_menu());
    }

    /// Letters fire alone or with Shift, never with Ctrl or Alt (Alt+S is
    /// kept free for the command search, G7).
    #[test]
    fn letters_without_ctrl_or_alt() {
        use egui::Modifiers as M;
        assert!(letter_alias(M::NONE));
        assert!(letter_alias(M::SHIFT));
        assert!(!letter_alias(M::ALT));
        assert!(!letter_alias(M::CTRL));
        assert!(!letter_alias(M::COMMAND));
        assert!(!letter_alias(M::ALT | M::SHIFT));
        assert!(!letter_alias(M::CTRL | M::ALT));
        assert!(!letter_alias(M::MAC_CMD));
    }

    /// Enter / Ctrl+Enter reach the panel of a feature command; commands
    /// with the shared flow and sketch tools take them elsewhere.
    #[test]
    fn panel_keys() {
        let fillet = open("Fillet / Chamfer");
        assert_eq!(panel_key(&fillet, false, false), None);
        assert_eq!(panel_key(&fillet, true, false), Some(PanelKey::Enter));
        assert_eq!(panel_key(&fillet, true, true), Some(PanelKey::Ok));
        let idle = OpenState::default();
        assert_eq!(panel_key(&idle, true, true), None);
        assert_eq!(panel_key(&idle, true, false), None);
        let shell = OpenState {
            flow: true,
            ..open("Shell")
        };
        assert_eq!(panel_key(&shell, true, true), None);
        assert_eq!(panel_key(&shell, true, false), None);
        let dialog = OpenState {
            sketch_tool: true,
            ..open("Sketch Dialog")
        };
        assert_eq!(panel_key(&dialog, true, true), Some(PanelKey::Ok));
        assert_eq!(panel_key(&dialog, true, false), None);
    }

    /// Enter = Continue while an input is missing, else OK; Ctrl+Enter = OK
    /// only when the command can finish.
    #[test]
    fn panel_answers() {
        use PanelAnswer as A;
        assert_eq!(panel_answer(None, true), None);
        assert_eq!(panel_answer(Some(PanelKey::Ok), true), Some(A::Ok));
        assert_eq!(panel_answer(Some(PanelKey::Ok), false), None);
        assert_eq!(panel_answer(Some(PanelKey::Enter), true), Some(A::Ok));
        assert_eq!(
            panel_answer(Some(PanelKey::Enter), false),
            Some(A::Continue)
        );
    }
}
