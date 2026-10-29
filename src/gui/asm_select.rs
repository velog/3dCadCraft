//! Selection priorities and appearance overrides in an assembly
//! (`docs/spec/3.0-assembly-commands.md` §3.7, roadmap AU3): Component /
//! Part priority select occurrences (`AsmUi::sel`; Part priority selects a
//! part inside a subassembly as `AsmSub::Part`, spec 3.1a), Feature
//! priority and Select Faces and Edges select a feature, face or edge of
//! one (`AsmUi::sub`, also of a part inside a subassembly). The Appearance list of the Quick Access Toolbar gives the
//! selection an override that belongs to the assembly (`Occurrence::looks`).

use std::collections::HashMap;

use eframe::egui::{self, Color32, Pos2, Rect};

use super::CadCraftApp;
use super::material::FaceOver;
use crate::assembly::looks::AsmLookTarget;
use crate::assembly::relation::GeomRef;
use crate::command::selection::AsmPriority;
use crate::document::{Occurrence, feature_of_face};
use crate::materials::Appearance;

/// A feature, face or edge of an occurrence picked under Feature priority
/// or Select Faces and Edges.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum AsmSub {
    /// A feature of the occurrence's part (its id); `sub` is the path to a
    /// part inside a subassembly (empty: the occurrence's own part).
    Feature {
        occurrence: u32,
        id: String,
        sub: Vec<u32>,
    },
    /// A face or an edge (`GeomRef::Face` / `GeomRef::Edge`).
    Geom(GeomRef),
    /// A part inside subassembly `occurrence` (Part Priority), by its path.
    Part { occurrence: u32, sub: Vec<u32> },
}

impl AsmSub {
    /// The occurrence it lies on.
    pub fn occurrence(&self) -> u32 {
        match self {
            AsmSub::Feature { occurrence, .. } | AsmSub::Part { occurrence, .. } => *occurrence,
            AsmSub::Geom(g) => g.occurrence(),
        }
    }

    /// The path below the occurrence to the part it lies on.
    fn sub(&self) -> &[u32] {
        match self {
            AsmSub::Feature { sub, .. } | AsmSub::Part { sub, .. } => sub,
            AsmSub::Geom(g) => g.sub(),
        }
    }

    /// Whether face `name` of the part at `path` below occurrence `occ`
    /// belongs to it (edges have no faces: they are drawn as lines).
    fn covers(&self, occ: u32, path: &[u32], name: &str) -> bool {
        if self.occurrence() != occ || self.sub() != path {
            return false;
        }
        match self {
            AsmSub::Feature { id, .. } => feature_of_face(name) == id,
            AsmSub::Geom(GeomRef::Face { name: n, .. }) => n == name,
            AsmSub::Part { .. } => true,
            AsmSub::Geom(_) => false,
        }
    }

    /// The appearance override target (edges have none; parts inside a
    /// subassembly not yet: their overrides belong to the subassembly).
    fn look_target(&self) -> Option<AsmLookTarget> {
        if !self.sub().is_empty() {
            return None;
        }
        match self {
            AsmSub::Feature { occurrence, id, .. } => Some(AsmLookTarget::Feature {
                occurrence: *occurrence,
                id: id.clone(),
            }),
            AsmSub::Geom(GeomRef::Face {
                occurrence, name, ..
            }) => Some(AsmLookTarget::Face {
                occurrence: *occurrence,
                name: name.clone(),
            }),
            AsmSub::Geom(_) | AsmSub::Part { .. } => None,
        }
    }
}

/// Selected: blue; under the cursor: green (as faces in a part).
const SEL: [f32; 3] = [0.25, 0.5, 0.95];
const HOVER: [f32; 3] = [0.4, 0.82, 0.45];

impl CadCraftApp {
    /// The assembly's Select Priority.
    pub(super) fn asm_priority(&self) -> AsmPriority {
        self.select.asm_priority
    }

    /// Sets it; the selection's kind changes, so it is cleared.
    pub(super) fn set_asm_priority(&mut self, p: AsmPriority) {
        if self.select.asm_priority != p {
            self.asm.sel.clear();
            self.asm.sub.clear();
            self.asm.sub_hover = None;
            self.scene_dirty = true;
        }
        self.select.asm_priority = p;
    }

    /// The feature, face or edge the priority takes under the cursor.
    pub(super) fn asm_sub_under(&self, pos: Pos2, rect: Rect) -> Option<AsmSub> {
        match self.asm_priority() {
            AsmPriority::Feature => {
                let (part, f, _) = self.part_hit(pos, rect)?;
                let name = self.with_part(part.component, |p| {
                    p.body.face(f).and_then(|x| x.name.clone())
                })??;
                Some(AsmSub::Feature {
                    occurrence: part.top,
                    id: feature_of_face(&name).to_owned(),
                    sub: part.sub,
                })
            }
            // A part inside a subassembly (a first-level part is a
            // component: `AsmUi::sel`).
            AsmPriority::Part => {
                let (part, _, _) = self.part_hit(pos, rect)?;
                (!part.sub.is_empty()).then_some(AsmSub::Part {
                    occurrence: part.top,
                    sub: part.sub,
                })
            }
            AsmPriority::FacesEdges => match self.geom_under_pub(pos, rect)? {
                g @ (GeomRef::Face { .. } | GeomRef::Edge { .. }) => Some(AsmSub::Geom(g)),
                _ => None,
            },
            _ => None,
        }
    }

    /// A click under Feature / Faces and Edges priority: `under` replaces
    /// the selection, with Ctrl / Shift it is added or removed; a click on
    /// nothing clears it (not with Ctrl / Shift).
    pub(super) fn asm_sub_click(&mut self, under: Option<AsmSub>, add: bool) {
        self.asm.sel.clear();
        match under {
            Some(s) if add => {
                if let Some(k) = self.asm.sub.iter().position(|x| *x == s) {
                    self.asm.sub.remove(k);
                } else {
                    self.asm.sub.push(s);
                }
            }
            Some(s) => self.asm.sub = vec![s],
            None if !add => self.asm.sub.clear(),
            None => {}
        }
    }

    /// The appearances the assembly's overrides use, by name.
    fn asm_looks(&self) -> HashMap<String, Appearance> {
        let Some(a) = self.assembly() else {
            return HashMap::new();
        };
        a.occurrences
            .iter()
            .flat_map(|o| o.looks.names())
            .filter_map(|n| Some((n.to_owned(), a.appearance(n, &self.mat.lib)?)))
            .collect()
    }

    /// Adds occurrence `o` placed at `place` to the scene: its overrides,
    /// and the selection / prehighlight tint (whole, or the selected
    /// feature's / face's faces only).
    pub(super) fn add_occurrence_mesh(
        &self,
        g: &mut super::renderer::SceneGeometry,
        o: &Occurrence,
        place: &crate::kernel::Transform3,
        alpha: Option<f32>,
        looks: &HashMap<String, Appearance>,
    ) {
        let whole = if self.asm.sel.contains(&o.id) {
            Some(SEL)
        } else if self.asm.hover == Some(o.id) {
            Some(HOVER)
        } else {
            None
        };
        let subs: Vec<&AsmSub> = self
            .asm
            .sub
            .iter()
            .filter(|s| s.occurrence() == o.id)
            .collect();
        let hover = self
            .asm
            .sub_hover
            .as_ref()
            .or(self.asm.tree_hover.as_ref())
            .filter(|s| s.occurrence() == o.id);
        let tint = |path: &[u32], name: Option<&str>| {
            whole.or_else(|| {
                let n = name?;
                if subs.iter().any(|s| s.covers(o.id, path, n)) {
                    Some(SEL)
                } else if hover.is_some_and(|s| s.covers(o.id, path, n)) {
                    Some(HOVER)
                } else {
                    None
                }
            })
        };
        let over = |name: Option<&str>| FaceOver {
            look: o.looks.face_look(name).and_then(|n| looks.get(n).cloned()),
            tint: tint(&[], name),
        };
        // A subassembly (spec 3.1a): its parts where its own solve put
        // them; its appearance override covers them all, else theirs apply.
        let linked = self.linked();
        if let Some(sub) = crate::assembly::tree::assembly_of(&self.project, &linked, o.component) {
            let own = o
                .looks
                .appearance
                .as_deref()
                .and_then(|n| looks.get(n).cloned());
            for l in crate::assembly::tree::leaves(&self.project, &linked, o.component, sub) {
                let inner = &l.occurrence.looks;
                let leaf_over = |name: Option<&str>| FaceOver {
                    look: own.clone().or_else(|| {
                        inner
                            .face_look(name)
                            .and_then(|n| sub.appearance(n, &self.mat.lib))
                    }),
                    tint: tint(&l.path, name),
                };
                let at = place * l.world;
                self.with_part(l.component, |part| {
                    super::material::add_part_meshes_with(g, part, Some(&at), alpha, &leaf_over)
                });
            }
            return;
        }
        self.with_part(o.component, |part| {
            super::material::add_part_meshes_with(g, part, Some(place), alpha, &over)
        });
    }

    /// The scene's appearance table (one lookup per scene build).
    pub(super) fn asm_scene_looks(&self) -> HashMap<String, Appearance> {
        self.asm_looks()
    }

    /// Selected and prehighlighted edges (faces are tinted in the scene).
    pub(super) fn asm_sub_overlay(&self, painter: &egui::Painter, rect: Rect) {
        for s in &self.asm.sub {
            if let AsmSub::Geom(g @ GeomRef::Edge { .. }) = s {
                self.draw_geom(painter, rect, g, Color32::from_rgb(70, 130, 245));
            }
        }
        if let Some(AsmSub::Geom(g @ GeomRef::Edge { .. })) = &self.asm.sub_hover {
            self.draw_geom(painter, rect, g, Color32::from_rgb(110, 220, 120));
        }
    }

    /// The Select drop-down of the Quick Access Toolbar in an assembly.
    pub(super) fn asm_select_menu(&mut self, ui: &mut egui::Ui) {
        let p = self.asm_priority();
        let short = p.label().trim_end_matches(" Priority");
        let title = format!("Select: {}", short.trim_start_matches("Select "));
        ui.menu_button(title, |ui| {
            for q in AsmPriority::ALL {
                let r = ui
                    .add_enabled(q.available(), egui::RadioButton::new(p == q, q.label()))
                    .on_disabled_hover_text(
                        "Sketches are not shown in an assembly yet (spec 3.0 §3.7)",
                    );
                if r.clicked() {
                    self.set_asm_priority(q);
                    ui.close();
                }
            }
            ui.separator();
            let mut on = self.select.prehighlight;
            if ui.checkbox(&mut on, "Enable Prehighlight").changed() {
                self.select.prehighlight = on;
            }
            ui.separator();
            let any = !self.asm.sel.is_empty() || !self.asm.sub.is_empty();
            if ui
                .add_enabled(any, egui::Button::new("Clear Selection"))
                .clicked()
            {
                self.asm.sel.clear();
                self.asm.sub.clear();
                self.scene_dirty = true;
                ui.close();
            }
        })
        .response
        .on_hover_text("Selection priority in the assembly: what one click takes");
    }

    /// What the Appearance list acts on: the selected components, or the
    /// selected features and faces.
    fn asm_look_targets(&self) -> Vec<AsmLookTarget> {
        if !self.asm.sel.is_empty() {
            return self
                .asm
                .sel
                .iter()
                .map(|&i| AsmLookTarget::Occurrence(i))
                .collect();
        }
        self.asm
            .sub
            .iter()
            .filter_map(AsmSub::look_target)
            .collect()
    }

    /// The occurrences the selection touches (Clear).
    fn asm_selected_occurrences(&self) -> Vec<u32> {
        let mut v: Vec<u32> = self.asm.sel.clone();
        v.extend(self.asm.sub.iter().map(AsmSub::occurrence));
        v.sort_unstable();
        v.dedup();
        v
    }

    /// Gives `targets` appearance `name` (`None`: removes the override), one
    /// Undo step; nothing changes on an error (rule 5).
    pub(super) fn asm_set_look(&mut self, targets: &[AsmLookTarget], name: Option<&str>) {
        let Some(a) = self.assembly() else {
            return;
        };
        if targets.is_empty() {
            self.file_status = Some(Err(
                "Appearance: select components, features or faces first".into(),
            ));
            return;
        }
        // A part's own custom appearances can be given to other parts.
        let mut extra: Vec<Appearance> = Vec::new();
        for o in &a.occurrences {
            if let Some(v) = self.with_part(o.component, |p| p.looks.appearances.clone()) {
                extra.extend(v);
            }
        }
        let mut trial = a.clone();
        let mut changed = false;
        for t in targets {
            match trial.set_look(t, name, &self.mat.lib, &extra) {
                Ok(c) => changed |= c,
                Err(e) => {
                    self.file_status = Some(Err(format!("Appearance: {e}")));
                    return;
                }
            }
        }
        if changed {
            self.history.break_coalescing();
            self.edit_assembly("Appearance", |a| *a = trial);
            self.history.break_coalescing();
        }
        self.file_status = None;
    }

    /// Clear: removes the overrides of the selected components (nothing
    /// selected: of every component).
    pub(super) fn asm_clear_looks(&mut self) {
        let ids = self.asm_selected_occurrences();
        let Some(mut trial) = self.assembly().cloned() else {
            return;
        };
        let n = trial.clear_looks((!ids.is_empty()).then_some(&ids[..]));
        if n > 0 {
            self.edit_assembly("Clear Appearance", |a| *a = trial);
        }
        self.file_status = Some(Ok(format!("Clear: {n} override(s) removed")));
    }

    /// The Appearance list's title: the first target's override.
    fn asm_appearance_label(&self) -> String {
        let Some(a) = self.assembly() else {
            return String::new();
        };
        let targets = self.asm_look_targets();
        let name = |t: &AsmLookTarget| {
            let o = a.find(t.occurrence())?;
            match t {
                AsmLookTarget::Occurrence(_) => o.looks.appearance.clone(),
                AsmLookTarget::Feature { id, .. } => o.looks.features.get(id).cloned(),
                AsmLookTarget::Face { name, .. } => o.looks.faces.get(name).cloned(),
            }
        };
        match targets.first() {
            None => "-".into(),
            Some(t) => {
                let first = name(t);
                if targets.iter().any(|x| name(x) != first) {
                    return "Varies".into();
                }
                // * = some features or faces have their own (as in a part).
                let star = targets.iter().any(|t| {
                    matches!(t, AsmLookTarget::Occurrence(_))
                        && a.find(t.occurrence()).is_some_and(|o| {
                            !o.looks.features.is_empty() || !o.looks.faces.is_empty()
                        })
                });
                format!(
                    "{}{}",
                    if star { "*" } else { "" },
                    first.unwrap_or_else(|| "As Part".into())
                )
            }
        }
    }

    /// The Appearance list of the Quick Access Toolbar in an assembly (no
    /// Material list: the parts carry their materials).
    pub(super) fn asm_appearance_qat(&mut self, ui: &mut egui::Ui) {
        let targets = self.asm_look_targets();
        let mut pick = None;
        let mut clear = false;
        let doc = self
            .assembly()
            .map(|a| a.appearances.clone())
            .unwrap_or_default();
        ui.add_enabled_ui(!targets.is_empty(), |ui| {
            ui.menu_button(
                format!("Appearance: {}", self.asm_appearance_label()),
                |ui| {
                    ui.set_max_height(480.0);
                    pick = self.asm_appearance_items(ui, &doc);
                    ui.separator();
                    if ui.button("Clear...").clicked() {
                        clear = true;
                        ui.close();
                    }
                },
            )
            .response
            .on_hover_text(
                "Appearance override of the selected components, features or faces \
                 (kept in the assembly; the parts keep their own)",
            )
            .on_disabled_hover_text("Select components, features or faces first");
        });
        if let Some(n) = pick {
            self.asm_set_look(&targets, n.as_deref());
        }
        if clear {
            self.asm_clear_looks();
        }
    }

    /// The appearance entries with "As Part" (remove the override) on top.
    pub(super) fn asm_appearance_items(
        &self,
        ui: &mut egui::Ui,
        doc: &[Appearance],
    ) -> Option<Option<String>> {
        // `appearance_items` names its first entry for parts ("As
        // Material"); in an assembly removing the override shows the part.
        if ui
            .button("As Part")
            .on_hover_text("Remove the assembly's override: the part's own look")
            .clicked()
        {
            ui.close();
            return Some(None);
        }
        ui.separator();
        self.appearance_items_without_reset(ui, doc)
    }

    /// The view's menu under Feature priority / Select Faces and Edges:
    /// Constrain / Joint / Measure, Appearance, Clear Selection.
    pub(super) fn asm_sub_menu(&mut self, ui: &mut egui::Ui, under: Option<AsmSub>) {
        if let Some(s) = under
            && !self.asm.sub.contains(&s)
        {
            self.asm.sub = vec![s];
            self.asm.sel.clear();
            self.scene_dirty = true;
        }
        let ids = self.asm_selected_occurrences();
        if !ids.is_empty() {
            if ui.button("Constrain...").clicked() {
                self.asm_apply(super::ribbon::Action::AsmConstrain);
                self.menu_first_pick(&ids, false);
                ui.close();
            }
            if ui.button("Joint...").clicked() {
                self.asm_apply(super::ribbon::Action::AsmJoint);
                self.menu_first_pick(&ids, true);
                ui.close();
            }
            if ui.button("Measure").clicked() {
                self.apply(super::ribbon::Action::App(super::commands::AppCmd::Measure));
                ui.close();
            }
            ui.separator();
        }
        let targets = self.asm_look_targets();
        if !targets.is_empty() {
            let doc = self
                .assembly()
                .map(|a| a.appearances.clone())
                .unwrap_or_default();
            let mut pick = None;
            ui.menu_button("Appearance", |ui| {
                ui.set_max_height(480.0);
                pick = self.asm_appearance_items(ui, &doc);
            });
            if let Some(n) = pick {
                self.asm_set_look(&targets, n.as_deref());
                ui.close();
            }
        }
        if !self.asm.sub.is_empty() && ui.button("Clear Selection").clicked() {
            self.asm.sub.clear();
            self.scene_dirty = true;
            ui.close();
        }
        ui.separator();
        if ui.button("Zoom All").clicked() {
            self.fit_view();
            ui.close();
        }
    }
}
