//! Representations in the assembly's browser (`docs/spec/3.9-representations.md`):
//! the folders *Representations › View / Position* and *Model States*, their
//! menus, the Override window and the Model States table. The logic is
//! `assembly::reps`; here are the Undo steps, the solves and the camera.

use eframe::egui::{self, RichText};

use super::CadCraftApp;
use super::icons::Icon;
use super::valuefield::ExprValue;
use crate::assembly::relation::ConstraintKind;
use crate::assembly::reps::{DEFAULT_VIEW, OccOverride, PRIMARY, Representations, SavedCamera};
use crate::kernel::{Point3, Real};

/// Which list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepKind {
    /// View representations.
    View,
    /// Positional representations.
    Position,
    /// Model states.
    State,
}

impl RepKind {
    fn label(self) -> &'static str {
        match self {
            RepKind::View => "View",
            RepKind::Position => "Position",
            RepKind::State => "Model State",
        }
    }
}

/// What the Override window edits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum OverrideOf {
    /// A relationship: value and suppress.
    Relationship(u32),
    /// A component: grounded and offset.
    Occurrence(u32),
}

/// The Override window's fields.
#[derive(Clone, Debug)]
pub(super) struct OverrideUi {
    of: OverrideOf,
    use_value: bool,
    value: Real,
    use_suppress: bool,
    suppress: bool,
    use_ground: bool,
    ground: bool,
    use_offset: bool,
    offset: [Real; 3],
}

/// Window state of the representations.
#[derive(Clone, Debug, Default)]
pub(super) struct RepsUi {
    over: Option<OverrideUi>,
    /// The Model States table is open.
    table: bool,
    /// Show every component in the table, not only those that differ.
    table_all: bool,
    /// Renaming: list, id, text, focus asked for.
    rename: Option<(RepKind, u32, String, bool)>,
}

/// A browser click, run after the tree is drawn.
#[derive(Clone, Copy, Debug)]
enum Cmd {
    New(RepKind),
    Activate(RepKind, u32),
    Copy(RepKind, u32),
    Delete(RepKind, u32),
    Rename(RepKind, u32),
    Lock(u32),
    SaveCamera(u32),
    RestoreCamera(u32),
    AutoCamera(u32),
    AllVisible(bool),
    RemoveLooks,
    ToState(u32),
    Verify,
    Table,
    Scope,
}

/// (id, name, active, locked) of a row.
type Row = (u32, String, bool, bool);

impl CadCraftApp {
    /// Runs once a frame in an assembly: the active representations follow the
    /// live state.
    pub(super) fn reps_tick(&mut self) {
        let Some(a) = self.assembly_mut() else {
            return;
        };
        if a.reps.is_empty() {
            return;
        }
        let mut r = std::mem::take(&mut a.reps);
        r.sync(a);
        a.reps = r;
    }

    /// Runs `f` on the assembly's representations and the assembly, taken apart.
    fn with_reps<T>(
        a: &mut crate::document::AssemblyDocument,
        f: impl FnOnce(&mut Representations, &mut crate::document::AssemblyDocument) -> T,
    ) -> T {
        let mut r = std::mem::take(&mut a.reps);
        let out = f(&mut r, a);
        a.reps = r;
        out
    }

    /// The numbers of the present camera.
    fn saved_camera(&self) -> SavedCamera {
        let c = &self.camera;
        SavedCamera {
            target: [c.target.x, c.target.y, c.target.z],
            distance: c.distance,
            yaw: c.yaw,
            pitch: c.pitch,
            roll: c.roll,
        }
    }

    /// Goes to a stored camera, with the usual transition.
    fn go_to_camera(&mut self, s: SavedCamera) {
        let before = self.camera.clone();
        self.camera.target = Point3::new(s.target[0], s.target[1], s.target[2]);
        self.camera.distance = s.distance;
        self.camera.yaw = s.yaw;
        self.camera.pitch = s.pitch;
        self.camera.roll = s.roll;
        self.view_cube.animate(before, &mut self.camera);
    }

    /// A change of position or model state: solved on a copy, nothing changes
    /// when it does not solve (rule 5).
    fn reps_solved(
        &mut self,
        label: &str,
        what: RepKind,
        f: impl FnOnce(&mut Representations, &mut crate::document::AssemblyDocument),
    ) {
        let r = self.asm_try(label, |a| Self::with_reps(a, f));
        if let Err(e) = r {
            self.file_status = Some(Err(format!("{}: {e}", what.label())));
        }
        self.asm_refresh_status();
    }

    /// Ribbon: a new entry of `kind`.
    pub(super) fn new_representation(&mut self, kind: RepKind) {
        self.run_rep(Cmd::New(kind));
    }

    /// Ribbon: the Model States table.
    pub(super) fn open_state_table(&mut self) {
        self.run_rep(Cmd::Table);
    }

    /// Ribbon: Verify.
    pub(super) fn verify_positions(&mut self) {
        self.run_rep(Cmd::Verify);
    }

    fn run_rep(&mut self, c: Cmd) {
        match c {
            Cmd::New(RepKind::View) => {
                let camera = self.saved_camera();
                self.edit_assembly("New View Representation", |a| {
                    Self::with_reps(a, |r, a| {
                        let id = r.new_view(a);
                        if let Some(v) = r.views.iter_mut().find(|v| v.id == id) {
                            v.camera = Some(camera);
                        }
                    });
                });
            }
            Cmd::New(RepKind::Position) => {
                self.reps_solved(
                    "New Positional Representation",
                    RepKind::Position,
                    |r, a| {
                        r.new_position(a);
                    },
                );
            }
            Cmd::New(RepKind::State) => {
                self.reps_solved("New Model State", RepKind::State, |r, a| {
                    r.new_state(a);
                });
            }
            Cmd::Activate(RepKind::View, id) => {
                let camera = self.saved_camera();
                let mut go = None;
                self.edit_assembly("Activate View Representation", |a| {
                    Self::with_reps(a, |r, a| {
                        let old = r.active_view;
                        if let Some(v) = r.views.iter_mut().find(|v| v.id == old)
                            && v.auto_camera
                        {
                            v.camera = Some(camera);
                        }
                        if r.activate_view(id, a) {
                            go = r.active_view().and_then(|v| v.camera);
                        }
                    });
                });
                if let Some(s) = go {
                    self.go_to_camera(s);
                }
            }
            Cmd::Activate(RepKind::Position, id) => {
                self.reps_solved(
                    "Activate Positional Representation",
                    RepKind::Position,
                    |r, a| {
                        r.activate_position(id, a);
                    },
                );
            }
            Cmd::Activate(RepKind::State, id) => {
                self.reps_solved("Activate Model State", RepKind::State, |r, a| {
                    r.activate_state(id, a);
                });
            }
            Cmd::Copy(RepKind::View, id) => {
                self.edit_assembly("Copy View Representation", |a| {
                    Self::with_reps(a, |r, a| {
                        r.copy_view(id, a);
                    });
                });
            }
            Cmd::Copy(RepKind::Position, id) => {
                self.reps_solved(
                    "Copy Positional Representation",
                    RepKind::Position,
                    |r, a| {
                        r.copy_position(id, a);
                    },
                );
            }
            Cmd::Copy(RepKind::State, _) => {}
            Cmd::Delete(kind, id) => {
                self.edit_assembly("Delete Representation", |a| match kind {
                    RepKind::View => {
                        a.reps.delete_view(id);
                    }
                    RepKind::Position => {
                        a.reps.delete_position(id);
                    }
                    RepKind::State => {
                        a.reps.delete_state(id);
                    }
                });
            }
            Cmd::Rename(kind, id) => {
                let name = self.assembly().and_then(|a| match kind {
                    RepKind::View => a
                        .reps
                        .views
                        .iter()
                        .find(|v| v.id == id)
                        .map(|v| v.name.clone()),
                    RepKind::Position => a
                        .reps
                        .positions
                        .iter()
                        .find(|p| p.id == id)
                        .map(|p| p.name.clone()),
                    RepKind::State => a
                        .reps
                        .states
                        .iter()
                        .find(|s| s.id == id)
                        .map(|s| s.name.clone()),
                });
                if let Some(n) = name {
                    self.asm.reps.rename = Some((kind, id, n, false));
                }
            }
            Cmd::Lock(id) => self.edit_assembly("Lock View Representation", |a| {
                Self::with_reps(a, |r, a| r.ensure_views(a));
                if let Some(v) = a.reps.views.iter_mut().find(|v| v.id == id) {
                    v.locked = !v.locked;
                }
            }),
            Cmd::SaveCamera(id) => {
                let camera = self.saved_camera();
                self.edit_assembly("Save Current Camera", |a| {
                    Self::with_reps(a, |r, a| r.ensure_views(a));
                    if let Some(v) = a.reps.views.iter_mut().find(|v| v.id == id) {
                        v.camera = Some(camera);
                        v.auto_camera = false;
                    }
                });
            }
            Cmd::RestoreCamera(id) => {
                let s = self.assembly().and_then(|a| {
                    a.reps
                        .views
                        .iter()
                        .find(|v| v.id == id)
                        .and_then(|v| v.camera)
                });
                if let Some(s) = s {
                    self.go_to_camera(s);
                }
            }
            Cmd::AutoCamera(id) => self.edit_assembly("AutoSave Camera", |a| {
                Self::with_reps(a, |r, a| r.ensure_views(a));
                if let Some(v) = a.reps.views.iter_mut().find(|v| v.id == id) {
                    v.auto_camera = !v.auto_camera;
                }
            }),
            Cmd::AllVisible(on) => {
                self.edit_assembly(if on { "All Visible" } else { "All Hidden" }, |a| {
                    for o in &mut a.occurrences {
                        o.visible = on;
                    }
                });
            }
            Cmd::RemoveLooks => self.edit_assembly("Remove Appearance Overrides", |a| {
                for o in &mut a.occurrences {
                    o.looks = Default::default();
                }
            }),
            Cmd::ToState(id) => {
                self.reps_solved("Copy to Model State", RepKind::State, |r, a| {
                    r.view_to_state(id, a);
                });
            }
            Cmd::Verify => self.verify_now(),
            Cmd::Table => {
                self.asm.reps.table = true;
            }
            Cmd::Scope => self.edit_assembly("Toggle Edit Scope", |a| {
                a.reps.factory_scope = !a.reps.factory_scope;
            }),
        }
    }

    /// Verify: every positional representation is solved on a copy.
    fn verify_now(&mut self) {
        let Some(a) = self.assembly().cloned() else {
            return;
        };
        let mut bad = Vec::new();
        for p in &a.reps.positions {
            let c = a.reps.composed(&a, None, Some(p.id), None);
            if !self.solve_copy(&c, None).converged {
                bad.push(p.name.clone());
            }
        }
        self.file_status = Some(if bad.is_empty() {
            Ok(format!(
                "Verify: every positional representation solves ({})",
                a.reps.positions.len()
            ))
        } else {
            Err(format!("Verify: {} do not solve", bad.join(", ")))
        });
    }

    /// The browser's folders.
    pub(super) fn reps_tree(&mut self, ui: &mut egui::Ui) {
        let Some(a) = self.assembly() else {
            return;
        };
        let r = &a.reps;
        // [Primary] is always there, stored or not.
        let views: Vec<Row> = if r.views.is_empty() {
            vec![
                (PRIMARY, "Primary".into(), false, true),
                (DEFAULT_VIEW, "Default".into(), true, false),
            ]
        } else {
            r.views
                .iter()
                .map(|v| (v.id, v.name.clone(), v.id == r.active_view, v.locked))
                .collect()
        };
        let mut positions: Vec<Row> = vec![(
            PRIMARY,
            "Primary".into(),
            r.active_position == PRIMARY,
            false,
        )];
        positions.extend(
            r.positions
                .iter()
                .map(|p| (p.id, p.name.clone(), p.id == r.active_position, false)),
        );
        let states: Vec<Row> = if r.states.is_empty() {
            vec![(PRIMARY, "Primary".into(), true, false)]
        } else {
            r.states
                .iter()
                .map(|s| (s.id, s.name.clone(), s.id == r.active_state, false))
                .collect()
        };
        let cameras: std::collections::HashMap<u32, (bool, bool)> = r
            .views
            .iter()
            .map(|v| (v.id, (v.camera.is_some(), v.auto_camera)))
            .collect();
        let factory = r.factory_scope;
        let mut cmds = Vec::new();
        super::asm_browser::fold(ui, "asm_representations", false, |ui| {
            super::panels::tree_icon(ui, Icon::RepView);
            ui.label("Representations");
        })
        .body(|ui| {
            super::asm_browser::fold(ui, "asm_rep_view", false, |ui| {
                super::panels::tree_icon(ui, Icon::RepView);
                let l = ui.label("View");
                l.context_menu(|ui| {
                    if ui.button("New").clicked() {
                        cmds.push(Cmd::New(RepKind::View));
                        ui.close();
                    }
                });
            })
            .body(|ui| {
                for row in &views {
                    self.rep_row(
                        ui,
                        RepKind::View,
                        row,
                        cameras.get(&row.0).copied(),
                        &mut cmds,
                    );
                }
            });
            super::asm_browser::fold(ui, "asm_rep_position", false, |ui| {
                super::panels::tree_icon(ui, Icon::RepPosition);
                let l = ui.label("Position");
                l.context_menu(|ui| {
                    if ui.button("New").clicked() {
                        cmds.push(Cmd::New(RepKind::Position));
                        ui.close();
                    }
                    if ui.button("Verify").clicked() {
                        cmds.push(Cmd::Verify);
                        ui.close();
                    }
                });
            })
            .body(|ui| {
                for row in &positions {
                    self.rep_row(ui, RepKind::Position, row, None, &mut cmds);
                }
            });
        });
        super::asm_browser::fold(ui, "asm_model_states", false, |ui| {
            super::panels::tree_icon(ui, Icon::RepState);
            let l = ui.label("Model States");
            l.context_menu(|ui| {
                if ui.button("New Model State").clicked() {
                    cmds.push(Cmd::New(RepKind::State));
                    ui.close();
                }
                if ui.button("Edit Table...").clicked() {
                    cmds.push(Cmd::Table);
                    ui.close();
                }
            });
            if ui
                .small_button("+")
                .on_hover_text("New Model State")
                .clicked()
            {
                cmds.push(Cmd::New(RepKind::State));
            }
        })
        .body(|ui| {
            let (t, tip) = if factory {
                (
                    "Edit Scope: Factory",
                    "Suppressions go to every model state; click for the active one only",
                )
            } else {
                (
                    "Edit Scope: Member",
                    "Suppressions go to the active model state only; click for all of them",
                )
            };
            if ui.small_button(t).on_hover_text(tip).clicked() {
                cmds.push(Cmd::Scope);
            }
            for row in &states {
                self.rep_row(ui, RepKind::State, row, None, &mut cmds);
            }
        });
        for c in cmds {
            self.run_rep(c);
        }
    }

    /// One row of a list.
    fn rep_row(
        &mut self,
        ui: &mut egui::Ui,
        kind: RepKind,
        row: &Row,
        camera: Option<(bool, bool)>,
        cmds: &mut Vec<Cmd>,
    ) {
        let (id, name, active, locked) = row.clone();
        // Rename in place.
        if let Some((k, i, text, focused)) = &mut self.asm.reps.rename
            && *k == kind
            && *i == id
        {
            let e = ui.add(egui::TextEdit::singleline(text).desired_width(120.0));
            if !*focused {
                super::valuefield::focus_all(ui.ctx(), &e, text);
                *focused = true;
            }
            let done = e.lost_focus();
            let keep = ui.input(|i| !i.key_pressed(egui::Key::Escape));
            if done {
                let new = text.trim().to_owned();
                self.asm.reps.rename = None;
                if keep {
                    self.rename_rep(kind, id, &name, &new);
                }
            }
            return;
        }
        let shown = if id == PRIMARY {
            "[Primary]".to_owned()
        } else {
            name.clone()
        };
        ui.horizontal(|ui| {
            // A column of its own, so the names line up.
            ui.allocate_ui(egui::vec2(12.0, ui.available_height()), |ui| {
                ui.set_min_width(12.0);
                if active {
                    ui.label("✔");
                }
            });
            if locked {
                super::panels::tree_icon(ui, Icon::RepLock);
            }
            let r = ui.selectable_label(false, RichText::new(shown));
            if r.double_clicked() && !active {
                cmds.push(Cmd::Activate(kind, id));
            }
            r.context_menu(|ui| {
                if ui
                    .add_enabled(!active, egui::Button::new("Activate"))
                    .clicked()
                {
                    cmds.push(Cmd::Activate(kind, id));
                    ui.close();
                }
                if kind != RepKind::State
                    && (id != PRIMARY || kind == RepKind::View)
                    && ui.button("Copy").clicked()
                {
                    cmds.push(Cmd::Copy(kind, id));
                    ui.close();
                }
                if kind == RepKind::View {
                    if ui.button("Copy to Model State").clicked() {
                        cmds.push(Cmd::ToState(id));
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            id != PRIMARY,
                            egui::Button::new(if locked { "Unlock" } else { "Lock" }),
                        )
                        .clicked()
                    {
                        cmds.push(Cmd::Lock(id));
                        ui.close();
                    }
                    let (saved, auto) = camera.unwrap_or((false, true));
                    ui.menu_button("Camera View", |ui| {
                        if ui.button("Save Current Camera").clicked() {
                            cmds.push(Cmd::SaveCamera(id));
                            ui.close();
                        }
                        if ui
                            .add_enabled(saved, egui::Button::new("Restore Saved Camera"))
                            .clicked()
                        {
                            cmds.push(Cmd::RestoreCamera(id));
                            ui.close();
                        }
                        if ui
                            .add(egui::Button::selectable(auto, "AutoSave Camera"))
                            .clicked()
                        {
                            cmds.push(Cmd::AutoCamera(id));
                            ui.close();
                        }
                    });
                    if active && !locked {
                        ui.separator();
                        if ui.button("All Visible").clicked() {
                            cmds.push(Cmd::AllVisible(true));
                            ui.close();
                        }
                        if ui.button("All Hidden").clicked() {
                            cmds.push(Cmd::AllVisible(false));
                            ui.close();
                        }
                        if ui.button("Remove Appearance Overrides").clicked() {
                            cmds.push(Cmd::RemoveLooks);
                            ui.close();
                        }
                    }
                }
                if kind == RepKind::State && ui.button("Edit Table...").clicked() {
                    cmds.push(Cmd::Table);
                    ui.close();
                }
                if id != PRIMARY {
                    ui.separator();
                    if ui.button("Rename").clicked() {
                        cmds.push(Cmd::Rename(kind, id));
                        ui.close();
                    }
                    if ui
                        .add_enabled(!active, egui::Button::new("Delete"))
                        .clicked()
                    {
                        cmds.push(Cmd::Delete(kind, id));
                        ui.close();
                    }
                }
            });
        });
    }

    fn rename_rep(&mut self, kind: RepKind, id: u32, own: &str, new: &str) {
        if new == own {
            return;
        }
        let free = self.assembly().is_some_and(|a| match kind {
            RepKind::View => {
                Representations::name_free(new, own, a.reps.views.iter().map(|v| v.name.as_str()))
            }
            RepKind::Position => Representations::name_free(
                new,
                own,
                a.reps.positions.iter().map(|p| p.name.as_str()),
            ),
            RepKind::State => {
                Representations::name_free(new, own, a.reps.states.iter().map(|s| s.name.as_str()))
            }
        });
        if !free {
            self.file_status = Some(Err(format!("{}: the name {new} is taken", kind.label())));
            return;
        }
        let new = new.to_owned();
        self.edit_assembly("Rename Representation", |a| match kind {
            RepKind::View => {
                if let Some(v) = a.reps.views.iter_mut().find(|v| v.id == id) {
                    v.name = new;
                }
            }
            RepKind::Position => {
                if let Some(p) = a.reps.positions.iter_mut().find(|p| p.id == id) {
                    p.name = new;
                }
            }
            RepKind::State => {
                if let Some(s) = a.reps.states.iter_mut().find(|s| s.id == id) {
                    s.name = new;
                }
            }
        });
    }

    /// The active position differs from [Primary]: a relationship's row is bold
    /// and shows the value in parentheses.
    pub(super) fn rel_override_text(&self, id: u32) -> Option<String> {
        let a = self.assembly()?;
        let p = a.reps.active_position()?;
        let o = p.relationships.get(&id)?;
        let c = a.constraint(id)?;
        Some(match o.value {
            Some(v) if is_angle(c.kind) => format!("({:.4}°)", v.to_degrees()),
            Some(v) => format!("({v:.4} mm)"),
            None => "(override)".to_owned(),
        })
    }

    /// The same for a component's row.
    pub(super) fn occ_overridden(&self, id: u32) -> bool {
        self.assembly()
            .and_then(|a| a.reps.active_position())
            .is_some_and(|p| p.occurrences.contains_key(&id))
    }

    /// Adding or deleting components and relationships belongs in [Primary]:
    /// `true` (and a message) when another position is active.
    pub(super) fn reps_refuse_structure(&mut self) -> bool {
        if self.can_override() {
            self.file_status = Some(Err(
                "Position: activate [Primary] to add or delete components and relationships"
                    .to_owned(),
            ));
            return true;
        }
        false
    }

    /// Whether Override can be used: a positional representation other than
    /// [Primary] is active.
    pub(super) fn can_override(&self) -> bool {
        self.assembly()
            .is_some_and(|a| !a.reps.is_primary_position())
    }

    /// The Override entries of a relationship's or a component's menu.
    pub(super) fn override_menu(&mut self, ui: &mut egui::Ui, of: OverrideOf) {
        let can = self.can_override();
        if ui
            .add_enabled(can, egui::Button::new("Override..."))
            .on_disabled_hover_text("Activate a positional representation other than [Primary]")
            .clicked()
        {
            self.open_override(of);
            ui.close();
        }
        if can && self.has_override(of) && ui.button("Remove Override").clicked() {
            self.remove_override(of);
            ui.close();
        }
    }

    fn has_override(&self, of: OverrideOf) -> bool {
        let Some(p) = self.assembly().and_then(|a| a.reps.active_position()) else {
            return false;
        };
        match of {
            OverrideOf::Relationship(id) => p.relationships.contains_key(&id),
            OverrideOf::Occurrence(id) => p.occurrences.contains_key(&id),
        }
    }

    fn open_override(&mut self, of: OverrideOf) {
        let Some(a) = self.assembly() else {
            return;
        };
        let p = a.reps.active_position();
        let mut u = OverrideUi {
            of,
            use_value: false,
            value: 0.0,
            use_suppress: false,
            suppress: false,
            use_ground: false,
            ground: false,
            use_offset: false,
            offset: [0.0; 3],
        };
        match of {
            OverrideOf::Relationship(id) => {
                let Some(c) = a.constraint(id) else {
                    return;
                };
                u.value = if is_angle(c.kind) {
                    c.value.to_degrees()
                } else {
                    c.value
                };
                u.suppress = c.suppressed;
                let o = p.and_then(|p| p.relationships.get(&id));
                u.use_value = o.is_some_and(|o| o.value.is_some());
                u.use_suppress = o.is_some_and(|o| o.suppressed.is_some());
            }
            OverrideOf::Occurrence(id) => {
                let Some(x) = a.find(id) else {
                    return;
                };
                let v = x.transform.translation.vector;
                u.offset = [v.x, v.y, v.z];
                u.ground = x.grounded;
                let o = p.and_then(|p| p.occurrences.get(&id));
                u.use_ground = o.is_some_and(|o| o.grounded.is_some());
                u.use_offset = o.is_some_and(|o| o.offset.is_some());
            }
        }
        self.asm.reps.over = Some(u);
    }

    /// Writes the window's fields; `false` when it does not solve.
    fn apply_override(&mut self, u: &OverrideUi) -> bool {
        let u = u.clone();
        let r = self.asm_try("Override", move |a| {
            Self::with_reps(a, |r, a| {
                let base = r.primary.clone();
                let base_rel = r.active_state().map(|s| s.relationships.clone());
                match u.of {
                    OverrideOf::Relationship(id) => {
                        let Some(c) = a.constraint_mut(id) else {
                            return;
                        };
                        let angle = is_angle(c.kind);
                        c.value = if u.use_value {
                            if angle { u.value.to_radians() } else { u.value }
                        } else {
                            base.values.get(&id).copied().unwrap_or(c.value)
                        };
                        // A free joint holds an overridden value only when locked.
                        c.locked = if u.use_value {
                            c.kind.is_joint()
                        } else {
                            base.locked.get(&id).copied().unwrap_or(c.locked)
                        };
                        c.suppressed = if u.use_suppress {
                            u.suppress
                        } else {
                            base_rel.is_some_and(|s| s.contains(&id))
                        };
                    }
                    OverrideOf::Occurrence(id) => {
                        let Some(x) = a.find_mut(id) else {
                            return;
                        };
                        x.grounded = if u.use_ground {
                            u.ground
                        } else {
                            base.grounded.get(&id).copied().unwrap_or(x.grounded)
                        };
                        let active = r.active_position;
                        if let Some(p) = r.positions.iter_mut().find(|p| p.id == active) {
                            let e: &mut OccOverride = p.occurrences.entry(id).or_default();
                            e.offset = u.use_offset.then_some(u.offset);
                            if !u.use_offset && e.grounded.is_none() {
                                p.occurrences.remove(&id);
                            }
                        }
                        if u.use_offset {
                            x.transform.translation.vector =
                                crate::kernel::Vector3::new(u.offset[0], u.offset[1], u.offset[2]);
                        }
                    }
                }
            });
        });
        match r {
            Ok(()) => {
                self.asm_refresh_status();
                true
            }
            Err(e) => {
                self.file_status = Some(Err(format!("Override: {e}")));
                false
            }
        }
    }

    fn remove_override(&mut self, of: OverrideOf) {
        let u = OverrideUi {
            of,
            use_value: false,
            value: 0.0,
            use_suppress: false,
            suppress: false,
            use_ground: false,
            ground: false,
            use_offset: false,
            offset: [0.0; 3],
        };
        self.apply_override(&u);
    }

    /// The Override window and the Model States table.
    pub(super) fn reps_windows(&mut self, ctx: &egui::Context) {
        self.override_window(ctx);
        self.state_table_window(ctx);
    }

    fn override_window(&mut self, ctx: &egui::Context) {
        let Some(mut u) = self.asm.reps.over.clone() else {
            return;
        };
        let (title, angle) = match u.of {
            OverrideOf::Relationship(id) => {
                let c = self.assembly().and_then(|a| a.constraint(id));
                (
                    c.map_or(String::new(), |c| c.name.clone()),
                    c.is_some_and(|c| is_angle(c.kind)),
                )
            }
            OverrideOf::Occurrence(id) => (
                self.assembly()
                    .and_then(|a| a.find(id))
                    .map_or(String::new(), |o| o.name.clone()),
                false,
            ),
        };
        let mut open = true;
        let (mut ok, mut apply, mut cancel) = (false, false, false);
        let win = egui::Window::new(format!("Override Object: {title}"))
            .id(egui::Id::new("reps_override"))
            .open(&mut open)
            .resizable(false)
            .default_size([300.0, 160.0]);
        self.layout
            .place("reps_override", ctx.content_rect(), false, win)
            .show(ctx, |ui| {
                egui::Grid::new("reps_override_grid")
                    .num_columns(2)
                    .show(ui, |ui| match u.of {
                        OverrideOf::Relationship(_) => {
                            ui.checkbox(&mut u.use_value, "Value");
                            ui.add_enabled_ui(u.use_value, |ui| {
                                ui.add(ExprValue::new(&mut u.value).suffix(if angle {
                                    "°"
                                } else {
                                    " mm"
                                }));
                            });
                            ui.end_row();
                            ui.checkbox(&mut u.use_suppress, "Suppress");
                            ui.add_enabled_ui(u.use_suppress, |ui| {
                                ui.checkbox(&mut u.suppress, "Suppressed");
                            });
                            ui.end_row();
                        }
                        OverrideOf::Occurrence(_) => {
                            ui.checkbox(&mut u.use_ground, "Grounding");
                            ui.add_enabled_ui(u.use_ground, |ui| {
                                ui.checkbox(&mut u.ground, "Grounded");
                            });
                            ui.end_row();
                            ui.checkbox(&mut u.use_offset, "Offset");
                            ui.add_enabled_ui(u.use_offset, |ui| {
                                ui.horizontal(|ui| {
                                    for (k, n) in ["X", "Y", "Z"].iter().enumerate() {
                                        ui.label(*n);
                                        ui.add(ExprValue::new(&mut u.offset[k]).suffix(" mm"));
                                    }
                                });
                            });
                            ui.end_row();
                        }
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    ok = ui.button("OK").clicked();
                    apply = ui.button("Apply").clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });
        if ok || apply {
            let done = self.apply_override(&u);
            if ok && done {
                self.asm.reps.over = None;
                return;
            }
        }
        if cancel || !open {
            self.asm.reps.over = None;
            return;
        }
        self.asm.reps.over = Some(u);
    }

    fn state_table_window(&mut self, ctx: &egui::Context) {
        if !self.asm.reps.table {
            return;
        }
        let Some(a) = self.assembly() else {
            return;
        };
        let mut all = self.asm.reps.table_all;
        // (id, name, one column per state)
        type Col = (u32, String, Vec<bool>);
        let states: Vec<(u32, String)> = if a.reps.states.is_empty() {
            vec![(PRIMARY, "[Primary]".to_owned())]
        } else {
            a.reps
                .states
                .iter()
                .map(|s| {
                    let n = if s.id == PRIMARY {
                        "[Primary]".to_owned()
                    } else {
                        s.name.clone()
                    };
                    (s.id, n)
                })
                .collect()
        };
        let active = a.reps.active_state;
        let cols: Vec<Col> = a
            .occurrences
            .iter()
            .map(|o| {
                let cells = if a.reps.states.is_empty() {
                    vec![o.suppressed]
                } else {
                    a.reps
                        .states
                        .iter()
                        .map(|s| s.occurrences.contains(&o.id))
                        .collect()
                };
                (o.id, o.name.clone(), cells)
            })
            .collect();
        let shown: Vec<&Col> = cols
            .iter()
            .filter(|(_, _, c)| all || c.iter().any(|x| *x != c[0]))
            .collect();
        let mut toggle: Option<(u32, u32)> = None;
        let mut open = true;
        let win = egui::Window::new("Model States: Edit Table")
            .id(egui::Id::new("reps_table"))
            .open(&mut open)
            .resizable(true)
            .default_size([560.0, 300.0])
            .min_height(200.0);
        self.layout
            .place("reps_table", ctx.content_rect(), true, win)
            .show(ctx, |ui| {
                ui.checkbox(&mut all, "Show every component")
                    .on_hover_text("Off: only the components that differ between the model states");
                ui.separator();
                if shown.is_empty() {
                    ui.weak("The model states do not differ yet.");
                    return;
                }
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::Grid::new("reps_table_grid")
                            .striped(true)
                            .show(ui, |ui| {
                                ui.strong("Model State");
                                for (_, n, _) in &shown {
                                    ui.strong(n.as_str())
                                        .on_hover_text("Suppressed in the checked model states");
                                }
                                ui.end_row();
                                for (k, (sid, name)) in states.iter().enumerate() {
                                    if *sid == active {
                                        ui.strong(format!("✔ {name}"));
                                    } else {
                                        ui.label(name);
                                    }
                                    for (oid, _, cells) in &shown {
                                        let mut v = cells[k];
                                        if ui.checkbox(&mut v, "").changed() {
                                            toggle = Some((*sid, *oid));
                                        }
                                    }
                                    ui.end_row();
                                }
                            });
                    });
            });
        self.asm.reps.table_all = all;
        if !open {
            self.asm.reps.table = false;
        }
        if let Some((sid, oid)) = toggle {
            self.reps_solved("Edit Model State Table", RepKind::State, |r, a| {
                if sid == r.active_state {
                    if let Some(o) = a.find_mut(oid) {
                        o.suppressed = !o.suppressed;
                    }
                } else if let Some(s) = r.states.iter_mut().find(|s| s.id == sid)
                    && !s.occurrences.remove(&oid)
                {
                    s.occurrences.insert(oid);
                }
            });
        }
    }
}

/// Is a relationship's value an angle?
fn is_angle(k: ConstraintKind) -> bool {
    matches!(
        k,
        ConstraintKind::Angle | ConstraintKind::Rotational | ConstraintKind::Cylindrical
    )
}
