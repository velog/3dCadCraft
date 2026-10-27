//! The **Preset** row of the feature command panels
//! (`docs/spec/0.11-presets.md`): dropdown, Create new preset (+) and the
//! settings menu. Storage and the preset model are in [`crate::presets`];
//! this module only holds the per-command dropdown state and the hooks the
//! commands call:
//!
//! - [`CadCraftApp::preset_start`] when a command opens (New Default);
//! - [`CadCraftApp::preset_edit`] on Edit Feature (shows its preset);
//! - [`CadCraftApp::preset_row`] under the panel header;
//! - [`CadCraftApp::preset_stamp`] before OK / Apply and
//!   [`CadCraftApp::preset_used`] after a successful one.

use std::collections::HashMap;

use eframe::egui;

use super::{CadCraftApp, DraftDef};
use crate::part::Operation;
use crate::presets::{
    Active, LAST_USED, NO_PRESET, NewDefault, PresetData, PresetStore, Settings, SortOrder,
    apply_in,
};

/// A name being typed: a new preset or a rename.
#[derive(Debug)]
struct NameEdit {
    command: String,
    text: String,
    /// Renaming this preset (else creating one).
    rename: Option<String>,
    /// Focus was requested (once, see CLAUDE.md "text fields in popups").
    focused: bool,
}

/// Presets state of the GUI.
#[derive(Debug)]
pub(super) struct PresetUi {
    store: PresetStore,
    /// The dropdown entry of each command.
    active: HashMap<String, Active>,
    name_edit: Option<NameEdit>,
}

impl Default for PresetUi {
    fn default() -> Self {
        // Tests never touch the user's settings folder.
        let store = if cfg!(test) {
            PresetStore::new(None)
        } else {
            PresetStore::user()
        };
        Self {
            store,
            active: HashMap::new(),
            name_edit: None,
        }
    }
}

impl PresetUi {
    fn active(&self, command: &str) -> Active {
        self.active.get(command).cloned().unwrap_or_default()
    }

    /// A command opens in `part`: applies its New Default to `def`.
    /// Returns messages for the status bar.
    fn start<T: PresetData>(
        &mut self,
        part: &crate::part::PartDocument,
        command: &str,
        def: &mut T,
    ) -> Vec<String> {
        let mut msgs = Vec::new();
        if let Some(w) = self.store.take_warning(command) {
            msgs.push(format!("Presets: {w}"));
        }
        let (mut active, settings) = self.store.data(command).start();
        if let Some(s) = settings {
            match apply_in(part, def, &s) {
                Ok(w) if w.is_empty() => {}
                Ok(w) => msgs.push(format!("Presets: {}", w.join("; "))),
                Err(e) => {
                    msgs.push(format!("{command}: {e}"));
                    active = Active::NoPreset;
                }
            }
        }
        self.active.insert(command.to_owned(), active);
        self.name_edit = None;
        msgs
    }

    /// Edit Feature: the preset the feature was made with (No Preset if
    /// none or it no longer exists); the values stay the feature's own.
    fn edit<T: PresetData>(&mut self, command: &str, def: &T) {
        let active = def
            .preset_name()
            .filter(|n| self.store.data(command).find(n).is_some())
            .map_or(Active::NoPreset, |n| Active::Named(n.to_owned()));
        self.active.insert(command.to_owned(), active);
        self.name_edit = None;
    }

    /// The feature records the chosen preset's name.
    fn stamp<T: PresetData>(&self, command: &str, def: &mut T) {
        def.set_preset_name(match self.active(command) {
            Active::Named(n) => Some(n),
            _ => None,
        });
    }

    /// OK / Apply succeeded: Last Used (new features only) and the
    /// preset's use time are saved.
    fn used<T: PresetData>(&mut self, command: &str, def: &T, new: bool) -> Result<(), String> {
        let active = self.active(command);
        let data = self.store.data_mut(command);
        if new {
            data.record_last_used(def.capture());
        }
        if let Active::Named(n) = active {
            data.mark_used(&n);
        }
        self.store.save(command)
    }
}

/// What the dropdown / menu asked for this frame.
enum RowAction {
    Choose(Active),
    StartCreate,
    StartRename(String),
    SaveCurrent(String),
    Delete(String),
    NewDefault(NewDefault),
    Sort(SortOrder),
    ShowBuiltins(bool),
    FinishName,
    CancelName,
}

impl CadCraftApp {
    /// Shows a presets warning or error in the status bar.
    fn preset_status(&mut self, msg: String) {
        self.file_status = Some(Err(format!("Presets: {msg}")));
    }

    /// Writes the command's presets, reporting a failure.
    fn preset_save(&mut self, command: &str) {
        if let Err(e) = self.presets.store.save(command) {
            self.preset_status(e);
        }
    }

    /// Applies `settings` to `def` in the open part; `false` (and a message)
    /// when they do not fit.
    fn preset_apply<T: PresetData>(&mut self, command: &str, def: &mut T, s: &Settings) -> bool {
        match apply_in(self.part(), def, s) {
            Ok(w) => {
                if !w.is_empty() {
                    self.preset_status(w.join("; "));
                }
                true
            }
            Err(e) => {
                self.file_status = Some(Err(format!("{command}: {e}")));
                false
            }
        }
    }

    /// A command opens: the dropdown and the values follow the command's
    /// New Default (Last Used if it was used before, else No Preset).
    pub(super) fn preset_start<T: PresetData>(&mut self, command: &str, def: &mut T) {
        let mut p = std::mem::take(&mut self.presets);
        let msgs = p.start(self.part(), command, def);
        self.presets = p;
        if !msgs.is_empty() {
            self.file_status = Some(Err(msgs.join("; ")));
        }
    }

    /// Edit Feature: the dropdown shows the preset the feature was made with.
    pub(super) fn preset_edit<T: PresetData>(&mut self, command: &str, def: &T) {
        self.presets.edit(command, def);
    }

    /// Before OK / Apply: the feature records the chosen preset's name.
    pub(super) fn preset_stamp<T: PresetData>(&self, command: &str, def: &mut T) {
        self.presets.stamp(command, def);
    }

    /// After a successful OK / Apply (`new`: a new feature, not an edit).
    pub(super) fn preset_used<T: PresetData>(&mut self, command: &str, def: &T, new: bool) {
        if let Err(e) = self.presets.used(command, def, new) {
            self.preset_status(e);
        }
    }

    /// The Preset row under a command panel's header. Returns whether the
    /// values in `def` changed.
    pub(super) fn preset_row<T: PresetData>(
        &mut self,
        ui: &mut egui::Ui,
        command: &str,
        def: &mut T,
    ) -> bool {
        let active = self.presets.active(command);
        let data = self.presets.store.data(command).clone();
        let modified = match &active {
            Active::Named(n) => data.find(n).is_some_and(|p| def.differs_from(&p.settings)),
            _ => false,
        };
        let user_named = match &active {
            Active::Named(n) => data.find(n).is_some_and(|p| !p.builtin),
            _ => false,
        };
        let shown = match &active {
            Active::NoPreset => NO_PRESET.to_owned(),
            Active::LastUsed => LAST_USED.to_owned(),
            Active::Named(n) if modified => format!("*{n}"),
            Active::Named(n) => n.clone(),
        };
        let mut action = None;
        ui.horizontal(|ui| {
            ui.label("Preset");
            egui::ComboBox::from_id_salt(("preset", command))
                .selected_text(shown)
                .width(150.0)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(active == Active::NoPreset, NO_PRESET)
                        .on_hover_text("The command's own default values")
                        .clicked()
                    {
                        action = Some(RowAction::Choose(Active::NoPreset));
                    }
                    if data.last_used.is_some()
                        && ui
                            .selectable_label(active == Active::LastUsed, LAST_USED)
                            .on_hover_text("The values of the last OK / Apply")
                            .clicked()
                    {
                        action = Some(RowAction::Choose(Active::LastUsed));
                    }
                    let list = data.list();
                    let mut sep = true;
                    for p in &list {
                        if p.builtin && sep {
                            ui.separator();
                            sep = false;
                        }
                        let on = active == Active::Named(p.name.clone());
                        let r = ui.selectable_label(on, &p.name);
                        let r = if p.builtin {
                            r.on_hover_text("Built-in preset (read-only)")
                        } else {
                            r
                        };
                        if r.clicked() {
                            action = Some(RowAction::Choose(Active::Named(p.name.clone())));
                        }
                    }
                });
            if ui
                .small_button("+")
                .on_hover_text("Create new preset from the current values")
                .clicked()
            {
                action = Some(RowAction::StartCreate);
            }
            ui.menu_button("...", |ui| {
                let current = match &active {
                    Active::Named(n) if user_named => Some(n.clone()),
                    _ => None,
                };
                let tip = "Choose a preset of your own first (built-in presets are read-only)";
                if ui
                    .add_enabled(current.is_some(), egui::Button::new("Save Current"))
                    .on_disabled_hover_text(tip)
                    .clicked()
                    && let Some(n) = &current
                {
                    action = Some(RowAction::SaveCurrent(n.clone()));
                    ui.close();
                }
                if ui
                    .add_enabled(current.is_some(), egui::Button::new("Rename Current"))
                    .on_disabled_hover_text(tip)
                    .clicked()
                    && let Some(n) = &current
                {
                    action = Some(RowAction::StartRename(n.clone()));
                    ui.close();
                }
                if ui
                    .add_enabled(current.is_some(), egui::Button::new("Delete Current"))
                    .on_disabled_hover_text(tip)
                    .clicked()
                    && let Some(n) = &current
                {
                    action = Some(RowAction::Delete(n.clone()));
                    ui.close();
                }
                ui.separator();
                ui.menu_button(format!("New {command} Default"), |ui| {
                    for (v, label) in [
                        (NewDefault::LastUsed, "Last Used"),
                        (NewDefault::UseCurrent, "Use Current Preset"),
                        (NewDefault::NoPreset, "No Preset"),
                    ] {
                        if ui.radio(data.new_default == v, label).clicked() {
                            action = Some(RowAction::NewDefault(v));
                            ui.close();
                        }
                    }
                });
                ui.menu_button("Sort", |ui| {
                    for (v, label) in [
                        (SortOrder::RecentlyUsed, "Recently Used"),
                        (SortOrder::DateCreated, "Date created"),
                        (SortOrder::Alphabetical, "Alpha-Numeric"),
                        (SortOrder::LastModified, "Last modified"),
                    ] {
                        if ui.radio(data.sort == v, label).clicked() {
                            action = Some(RowAction::Sort(v));
                            ui.close();
                        }
                    }
                });
                let mut show = data.show_builtins;
                if ui.checkbox(&mut show, "Show built-in presets").changed() {
                    action = Some(RowAction::ShowBuiltins(show));
                }
            })
            .response
            .on_hover_text("Preset settings");
        });
        // The name field of Create new preset / Rename Current.
        if let Some(edit) = self
            .presets
            .name_edit
            .as_mut()
            .filter(|e| e.command == command)
        {
            ui.horizontal(|ui| {
                ui.label(if edit.rename.is_some() {
                    "Rename"
                } else {
                    "Name"
                });
                let r = ui.add(egui::TextEdit::singleline(&mut edit.text).desired_width(120.0));
                if !edit.focused {
                    super::valuefield::focus_all(ui.ctx(), &r, &edit.text);
                    edit.focused = true;
                }
                let (enter, esc) = ui.input_mut(|i| {
                    (
                        i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                        i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
                    )
                });
                if ui.small_button("OK").clicked() || (enter && (r.has_focus() || r.lost_focus())) {
                    action = Some(RowAction::FinishName);
                } else if ui.small_button("Cancel").clicked() || esc {
                    action = Some(RowAction::CancelName);
                }
            });
        }
        let Some(action) = action else {
            return false;
        };
        let mut changed = false;
        match action {
            RowAction::Choose(a) => {
                let s = self.presets.store.data(command).settings_of(&a);
                let ok = match &s {
                    Some(s) => self.preset_apply(command, def, s),
                    None => true,
                };
                if ok {
                    changed = s.is_some();
                    if let Active::Named(n) = &a {
                        self.presets.store.data_mut(command).mark_used(n);
                        self.preset_save(command);
                    }
                    self.presets.active.insert(command.to_owned(), a);
                }
            }
            RowAction::StartCreate => {
                self.presets.name_edit = Some(NameEdit {
                    command: command.to_owned(),
                    text: self.presets.store.data(command).default_name(),
                    rename: None,
                    focused: false,
                });
            }
            RowAction::StartRename(n) => {
                self.presets.name_edit = Some(NameEdit {
                    command: command.to_owned(),
                    text: n.clone(),
                    rename: Some(n),
                    focused: false,
                });
            }
            RowAction::FinishName => {
                if let Some(edit) = self.presets.name_edit.take() {
                    let data = self.presets.store.data_mut(command);
                    let done = match &edit.rename {
                        Some(old) => data.rename(old, &edit.text),
                        None => data.create(&edit.text, def.capture()),
                    };
                    match done {
                        Ok(name) => {
                            self.presets
                                .active
                                .insert(command.to_owned(), Active::Named(name));
                            self.preset_save(command);
                            self.file_status = None;
                        }
                        Err(e) => {
                            // Keep the field open to correct the name.
                            self.presets.name_edit = Some(edit);
                            self.preset_status(e);
                        }
                    }
                }
            }
            RowAction::CancelName => self.presets.name_edit = None,
            RowAction::SaveCurrent(n) => {
                let s = def.capture();
                match self.presets.store.data_mut(command).save_current(&n, s) {
                    Ok(()) => self.preset_save(command),
                    Err(e) => self.preset_status(e),
                }
            }
            RowAction::Delete(n) => match self.presets.store.data_mut(command).delete(&n) {
                Ok(()) => {
                    self.presets
                        .active
                        .insert(command.to_owned(), Active::NoPreset);
                    self.preset_save(command);
                }
                Err(e) => self.preset_status(e),
            },
            RowAction::NewDefault(v) => {
                self.presets.store.data_mut(command).new_default = v;
                self.preset_save(command);
            }
            RowAction::Sort(v) => {
                self.presets.store.data_mut(command).sort = v;
                self.preset_save(command);
            }
            RowAction::ShowBuiltins(v) => {
                self.presets.store.data_mut(command).show_builtins = v;
                self.preset_save(command);
            }
        }
        changed
    }

    /// [`Self::preset_start`] for Extrude / Revolve.
    pub(super) fn draft_preset_start(&mut self, def: &mut DraftDef) {
        let cmd = def.command();
        match def {
            DraftDef::Extrude(e) => {
                self.preset_start(cmd, e);
                keep_predicted_operation(&self.part().body, &mut e.operation);
            }
            DraftDef::Revolve(r) => {
                self.preset_start(cmd, r);
                keep_predicted_operation(&self.part().body, &mut r.operation);
            }
        }
    }

    /// [`Self::preset_edit`] for Extrude / Revolve.
    pub(super) fn draft_preset_edit(&mut self, def: &DraftDef) {
        match def {
            DraftDef::Extrude(e) => self.preset_edit(def.command(), e),
            DraftDef::Revolve(r) => self.preset_edit(def.command(), r),
        }
    }

    /// [`Self::preset_stamp`] for Extrude / Revolve.
    pub(super) fn draft_preset_stamp(&self, def: &mut DraftDef) {
        let cmd = def.command();
        match def {
            DraftDef::Extrude(e) => self.preset_stamp(cmd, e),
            DraftDef::Revolve(r) => self.preset_stamp(cmd, r),
        }
    }

    /// [`Self::preset_used`] for Extrude / Revolve.
    pub(super) fn draft_preset_used(&mut self, def: &DraftDef, new: bool) {
        match def {
            DraftDef::Extrude(e) => self.preset_used(def.command(), e, new),
            DraftDef::Revolve(r) => self.preset_used(def.command(), r, new),
        }
    }

    /// The Preset row of the Extrude / Revolve dialog.
    pub(super) fn extrude_preset_row(&mut self, ui: &mut egui::Ui) {
        let Some(mut def) = self.extrude.as_ref().map(|d| d.def.clone()) else {
            return;
        };
        let cmd = def.command();
        let changed = match &mut def {
            DraftDef::Extrude(e) => {
                let c = self.preset_row(ui, cmd, e);
                if c {
                    keep_predicted_operation(&self.part().body, &mut e.operation);
                }
                c
            }
            DraftDef::Revolve(r) => {
                let c = self.preset_row(ui, cmd, r);
                if c {
                    keep_predicted_operation(&self.part().body, &mut r.operation);
                }
                c
            }
        };
        if changed && let Some(d) = self.extrude.as_mut() {
            d.def = def;
        }
    }
}

/// Without a solid in the part a preset's Join / Cut / Intersect cannot
/// work: the predicted New Solid stays (Inventor: Predict Boolean overrides
/// the preset).
fn keep_predicted_operation(body: &crate::kernel::BRep, op: &mut Operation) {
    if Operation::default_for(body) == Operation::NewSolid {
        *op = Operation::NewSolid;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::part::{ExtrudeFeature, Feature, PartDocument, ShellDirection, ShellFeature};

    #[test]
    fn a_command_starts_with_last_used_or_no_preset() {
        let part = PartDocument::default();
        let mut ui = PresetUi::default();
        let mut d = ShellFeature::new(2.0);
        assert!(ui.start(&part, "Shell", &mut d).is_empty());
        assert_eq!(ui.active("Shell"), Active::NoPreset);
        assert_eq!(d.thickness, 2.0);
        // OK with 3 mm Outside: the next Shell starts with them.
        let mut used = ShellFeature::new(3.0);
        used.direction = ShellDirection::Outside;
        ui.stamp("Shell", &mut used);
        assert_eq!(used.preset, None, "No Preset records no name");
        ui.used("Shell", &used, true).unwrap();
        let mut d = ShellFeature::new(2.0);
        ui.start(&part, "Shell", &mut d);
        assert_eq!(ui.active("Shell"), Active::LastUsed);
        assert_eq!((d.thickness, d.direction), (3.0, ShellDirection::Outside));
        ui.stamp("Shell", &mut d);
        assert_eq!(d.preset, None, "Last Used records no name");
        // Edit Feature OKs do not change Last Used.
        ui.used("Shell", &ShellFeature::new(9.0), false).unwrap();
        let mut d = ShellFeature::new(2.0);
        ui.start(&part, "Shell", &mut d);
        assert_eq!(d.thickness, 3.0);
        // New Shell Default = No Preset.
        ui.store.data_mut("Shell").new_default = NewDefault::NoPreset;
        let mut d = ShellFeature::new(2.0);
        ui.start(&part, "Shell", &mut d);
        assert_eq!(ui.active("Shell"), Active::NoPreset);
        assert_eq!(d.thickness, 2.0);
    }

    #[test]
    fn edit_feature_shows_the_preset_it_was_made_with() {
        let mut ui = PresetUi::default();
        ui.active
            .insert("Shell".into(), Active::Named("Shell 1 mm Outside".into()));
        let mut f = ShellFeature::new(1.0);
        f.direction = ShellDirection::Outside;
        ui.stamp("Shell", &mut f);
        assert_eq!(f.preset.as_deref(), Some("Shell 1 mm Outside"));
        ui.used("Shell", &f, true).unwrap();
        // The name survives a save / load of the feature.
        let text = serde_json::to_string(&Feature::Shell(f.clone())).unwrap();
        let Ok(Feature::Shell(f)) = serde_json::from_str(&text) else {
            panic!("not a shell")
        };
        // Another command in between.
        ui.active.insert("Shell".into(), Active::NoPreset);
        ui.edit("Shell", &f);
        assert_eq!(
            ui.active("Shell"),
            Active::Named("Shell 1 mm Outside".into())
        );
        // A preset that no longer exists: No Preset.
        let mut g = f.clone();
        g.preset = Some("Gone".into());
        ui.edit("Shell", &g);
        assert_eq!(ui.active("Shell"), Active::NoPreset);
        // A feature without a preset (old file, Last Used): No Preset.
        g.preset = None;
        ui.edit("Shell", &g);
        assert_eq!(ui.active("Shell"), Active::NoPreset);
    }

    #[test]
    fn use_current_preset_starts_with_the_chosen_one() {
        let part = PartDocument::default();
        let mut ui = PresetUi::default();
        let data = ui.store.data_mut("Extrude");
        data.new_default = NewDefault::UseCurrent;
        let mut src = ExtrudeFeature::new(Vec::new(), 33.0);
        src.taper = 0.1;
        data.create("Deep", src.capture()).unwrap();
        let mut e = ExtrudeFeature::new(Vec::new(), 10.0);
        ui.start(&part, "Extrude", &mut e);
        assert_eq!(ui.active("Extrude"), Active::Named("Deep".into()));
        assert_eq!((e.distance, e.taper), (33.0, 0.1));
        ui.stamp("Extrude", &mut e);
        assert_eq!(e.preset.as_deref(), Some("Deep"));
    }

    #[test]
    fn an_empty_part_keeps_new_solid() {
        let part = PartDocument::default();
        let mut op = Operation::Cut;
        keep_predicted_operation(&part.body, &mut op);
        assert_eq!(op, Operation::NewSolid, "no body to cut");
    }
}
