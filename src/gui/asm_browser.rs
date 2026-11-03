//! The assembly's browser tree (`docs/spec/3.1-assemblies.md` §4), as in
//! Inventor: every node folds open and shut — *Relationships* (all
//! constraints), the assembly's *Origin*, and each component with its own
//! *Origin* (planes, axes, centre point), its part's work features and the
//! constraints that use it. A row of origin or work geometry lights up in
//! the view under the cursor and, while Constrain is open, a click on it is
//! a selection (the planes are not in the view, as in Inventor).
//!
//! AU5 (`docs/spec/3.0-assembly-commands.md` §3.9): Assembly View (the
//! relationships under each component) or Modeling View (each component's
//! part features instead), a search field, Display Preferences filters and a
//! visibility button per component.
//!
//! A subassembly's node lists its own components and relationships
//! (`docs/spec/3.1a-subassemblies.md` S4): read-only, greyed eyes; a click
//! selects a part inside it, a double-click opens its document.

use eframe::egui::{self, RichText};

use super::CadCraftApp;
use super::icons::Icon;
use crate::assembly::relation::{ASSEMBLY, GeomRef, OriginItem};
use crate::part::Feature;
use serde::{Deserialize, Serialize};

/// The assembly browser's view and filters (Application Options, persisted
/// in `AppOptions::asm_browser`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AsmBrowserOpts {
    /// Modeling View: the parts' features under the components, the
    /// relationships only in their folder.
    pub modeling: bool,
    /// Hide Work Features (and the Origin folders).
    pub hide_work: bool,
    /// Hide Fully Constrained: only components with DOF left.
    pub hide_fully_constrained: bool,
    /// Show Extended Names: a feature's operation and main value.
    pub extended_names: bool,
    /// Show Constraint State: a dot per component (filled = 0 DOF).
    pub constraint_state: bool,
}

/// A feature of a component's part, as the tree lists it.
struct FeatureRow {
    k: usize,
    id: String,
    name: String,
    icon: Icon,
    about: String,
    suppressed: bool,
    work: bool,
    sketches: Vec<String>,
}

/// The features of `p` in history order.
/// The tooltip of a component in a file of its own.
const LINKED_FILE_TIP: &str = "A linked part file: changes are saved to that file";

fn feature_rows(p: &crate::part::PartDocument) -> Vec<FeatureRow> {
    p.features
        .iter()
        .enumerate()
        .map(|(k, f)| {
            let (icon, about) = super::panels::feature_look(f);
            let ids = f.sketch_ids();
            FeatureRow {
                k,
                id: f.id().to_owned(),
                name: f.name().to_owned(),
                icon,
                about,
                suppressed: p.is_suppressed(k),
                work: matches!(f, Feature::Work(_)),
                sketches: p
                    .sketches
                    .iter()
                    .filter(|s| ids.contains(&s.id.as_str()))
                    .map(|s| s.name.clone())
                    .collect(),
            }
        })
        .collect()
}

/// What a click in the tree asked for (done after drawing).
/// A folder row's command.
enum FolderCmd {
    Rename(u32, String),
    Delete(u32),
}

enum TreeCmd {
    Select(u32, bool),
    /// A feature row (occurrence, feature id), Ctrl adds.
    SelectFeature(u32, String, bool),
    /// Open the part with feature `k` selected.
    OpenFeature(u32, usize),
    /// Zoom to a feature of a component.
    FindFeature(u32, String),
    /// The assembly's appearance override of a feature.
    FeatureLook(u32, String, Option<String>),
    ToggleVisible(u32),
    /// A component inside a subassembly (top occurrence, path), Ctrl adds.
    SelectNested(u32, Vec<u32>, bool),
    /// Open a component's document (a part or subassembly inside one).
    OpenDoc(crate::document::DocRef),
    /// Edit in place the part at a path below a top occurrence.
    Edit(u32, Vec<u32>),
    Pick(GeomRef),
    SelectConstraint(u32),
    EditConstraint(u32),
    SuppressConstraint(u32),
    DeleteConstraint(u32),
    Drive(u32),
}

impl CadCraftApp {
    /// The active assembly's tree in the browser.
    pub(super) fn assembly_tree(&mut self, ui: &mut egui::Ui, name: &str) {
        let linked = self.linked();
        let Some(a) = self.assembly() else {
            return;
        };
        // A pattern's copies are listed under its elements.
        let patterns = a.patterns.clone();
        let folders = a.folders.clone();
        let mut folder_cmd: Option<FolderCmd> = None;
        let occs: Vec<(u32, String, bool, bool, crate::document::DocRef)> = a
            .occurrences
            .iter()
            .filter(|o| a.pattern_of(o.id).is_none())
            // Frame members are listed under their frame.
            .filter(|o| {
                !a.frames
                    .iter()
                    .any(|f| f.members.iter().any(|m| m.occurrence == o.id))
            })
            .map(|o| (o.id, o.name.clone(), o.grounded, o.visible, o.component))
            .collect();
        // Suppressed: struck through; Enabled off: italic (Inventor).
        let looks: std::collections::HashMap<u32, (bool, bool)> = a
            .occurrences
            .iter()
            .map(|o| (o.id, (o.suppressed, o.enabled)))
            .collect();

        let all: Vec<u32> = a.constraints.iter().map(|c| c.id).collect();
        let constraint_names: std::collections::HashMap<u32, String> = a
            .constraints
            .iter()
            .map(|c| (c.id, c.name.to_lowercase()))
            .collect();
        let opts = self.asm_browser;
        let query = self.asm.search.trim().to_lowercase();
        let hit = |s: &str| query.is_empty() || s.to_lowercase().contains(&query);
        let named = |c: &u32| {
            query.is_empty() || constraint_names.get(c).is_some_and(|n| n.contains(&query))
        };
        self.asm.tree_hover = None;
        // Find in Browser: open and show that row (once).
        let find = self.asm.find_in_browser.take();
        let mut cmds = Vec::new();
        let mut pcmds = Vec::new();
        let mut fcmds = Vec::new();
        self.cons.browser_hover = None;
        let asm_doc = self.part_id;
        let mut iprops = false;
        let mut break_link = false;
        let link = self.asm_import_link().map(|l| l.path.clone());
        ui.horizontal(|ui| {
            super::panels::tree_icon(ui, Icon::Assembly);
            let r = ui.label(RichText::new(name).strong());
            let r = match &link {
                Some(p) => r.on_hover_text(format!("Follows {p} (Reference Model)")),
                None => r,
            };
            r.context_menu(|ui| {
                if ui.button("Properties...").clicked() {
                    iprops = true;
                    ui.close();
                }
                if let Some(p) = &link
                    && ui
                        .button("Break Link")
                        .on_hover_text(format!(
                            "Stop following {p}: the assembly keeps its components (Convert Model)"
                        ))
                        .clicked()
                {
                    break_link = true;
                    ui.close();
                }
            });
        });
        if iprops {
            self.open_iproperties(asm_doc);
        }
        if break_link {
            self.break_assembly_link();
        }
        self.asm_browser_bar(ui);
        ui.indent("asm_tree", |ui| {
            if query.is_empty() {
                self.reps_tree(ui);
            }
            let all: Vec<u32> = all.into_iter().filter(|c| named(c)).collect();
            if !all.is_empty() {
                fold(ui, "asm_relationships", true, |ui| {
                    super::panels::tree_icon(ui, Icon::Coincident);
                    ui.label("Relationships");
                })
                .body(|ui| {
                    for id in &all {
                        self.constraint_row(ui, *id, &mut cmds);
                    }
                });
            }
            // Assembly features with their participants (spec 3.10 §3.8).
            if query.is_empty() {
                self.asm_features_tree(ui);
            }
            if !opts.hide_work && query.is_empty() {
                self.origin_folder(ui, ASSEMBLY, &mut cmds);
            }
            type OccRow = (u32, String, bool, bool, crate::document::DocRef);
            let mut row = |this: &mut Self, ui: &mut egui::Ui, occ: &OccRow| {
                let (id, label, grounded, visible, component) = occ.clone();
                let (suppressed, enabled) = looks.get(&id).copied().unwrap_or((false, true));
                let dof = this.occurrence_dof(id);
                if opts.hide_fully_constrained && dof == 0 {
                    return;
                }
                let features = if opts.modeling {
                    this.with_part(component, feature_rows).unwrap_or_default()
                } else {
                    Vec::new()
                };
                let uses: Vec<u32> = this
                    .assembly()
                    .map(|a| {
                        a.constraints
                            .iter()
                            .filter(|c| c.uses(id))
                            .map(|c| c.id)
                            .collect()
                    })
                    .unwrap_or_default();
                // A search keeps a component that matches, or one with a
                // feature (Modeling View) or relationship (Assembly View)
                // that does; it opens to show them.
                let own_hit = hit(&label);
                let features: Vec<FeatureRow> = features
                    .into_iter()
                    .filter(|f| own_hit || hit(&f.name))
                    .filter(|f| !(f.work && opts.hide_work))
                    .collect();
                let uses: Vec<u32> = uses.into_iter().filter(|c| own_hit || named(c)).collect();
                let child_hit = if opts.modeling {
                    !features.is_empty()
                } else {
                    !uses.is_empty()
                };
                if !own_hit && !child_hit {
                    return;
                }
                let reveal = !query.is_empty() && child_hit && !own_hit;
                if find == Some(id) || reveal {
                    let key = ui.make_persistent_id(("asm_occ", id));
                    let mut st = egui::collapsing_header::CollapsingState::load_with_default_open(
                        ui.ctx(),
                        key,
                        false,
                    );
                    st.set_open(true);
                    st.store(ui.ctx());
                }
                if find == Some(id) {
                    cmds.push(TreeCmd::Select(id, false));
                }
                let sub =
                    crate::assembly::tree::assembly_of(&this.project, &linked, component).is_some();
                let header = fold(ui, ("asm_occ", id), false, |ui| {
                    if visibility_button(ui, visible) {
                        cmds.push(TreeCmd::ToggleVisible(id));
                    }
                    super::panels::tree_icon(ui, if sub { Icon::Assembly } else { Icon::Part });
                    if grounded {
                        super::panels::tree_icon(ui, Icon::Ground);
                    }
                    // A component in a file of its own says so in its tooltip.
                    let linked_file = component.library.is_some();
                    let mut text = RichText::new(label.clone());
                    if !visible || suppressed {
                        text = text.weak();
                    }
                    if suppressed {
                        text = text.strikethrough();
                    }
                    if !enabled {
                        text = text.italics();
                    }
                    // Overridden in the active positional representation.
                    if this.occ_overridden(id) {
                        text = text.strong();
                    }
                    let r = ui.selectable_label(this.asm.sel.contains(&id), text);
                    if opts.constraint_state {
                        state_dot(ui, dof);
                    }
                    if find == Some(id) {
                        r.scroll_to_me(Some(egui::Align::Center));
                    }
                    let r = if grounded {
                        r.on_hover_text("Grounded")
                    } else {
                        r
                    };
                    let r = if linked_file {
                        r.on_hover_text(LINKED_FILE_TIP)
                    } else {
                        r
                    };
                    if r.clicked() {
                        let add = ui.input(|i| i.modifiers.command || i.modifiers.shift);
                        cmds.push(TreeCmd::Select(id, add));
                    }
                    if r.double_clicked() {
                        cmds.push(TreeCmd::Edit(id, Vec::new()));
                    }
                    if r.hovered() {
                        this.asm.hover = Some(id);
                    }
                    r.context_menu(|ui| {
                        this.asm.menu_at = None;
                        if !this.asm.sel.contains(&id) {
                            this.asm.sel = vec![id];
                        }
                        let ids = this.asm.sel.clone();
                        this.occurrence_menu(ui, &ids);
                    });
                });
                header.body(|ui| {
                    if !opts.hide_work && query.is_empty() {
                        this.origin_folder(ui, id, &mut cmds);
                    }
                    if sub {
                        let mut stack = vec![asm_doc];
                        this.nested_rows(ui, id, component, &[], &mut stack, &mut cmds);
                    }
                    if opts.modeling {
                        // Modeling View: the part's features in history
                        // order (work features among them).
                        for f in &features {
                            this.feature_row(ui, id, f, opts.extended_names, &mut cmds);
                        }
                        return;
                    }
                    if !opts.hide_work {
                        // The part's work features.
                        let works = this
                            .with_part(component, |p| {
                                let mut v: Vec<(String, String)> = p
                                    .features
                                    .iter()
                                    .filter_map(|f| match f {
                                        Feature::Work(w) => Some((w.id.clone(), w.name.clone())),
                                        _ => None,
                                    })
                                    .filter(|(wid, _)| {
                                        p.work.planes.contains_key(wid)
                                            || p.work.axes.contains_key(wid)
                                            || p.work.points.contains_key(wid)
                                    })
                                    .collect();
                                v.sort();
                                v
                            })
                            .unwrap_or_default();
                        for (wid, wname) in works {
                            let g = GeomRef::Work {
                                sub: Vec::new(),
                                occurrence: id,
                                id: wid,
                            };
                            this.geom_row(ui, Icon::WorkPlane, &wname, g, &mut cmds);
                        }
                    }
                    // Assembly View: its relationships.
                    for c in &uses {
                        this.constraint_row(ui, *c, &mut cmds);
                    }
                });
            };
            // Folders first, each with its components; then the rest.
            let in_folder: Vec<u32> = folders.iter().flat_map(|f| f.items.clone()).collect();
            for f in &folders {
                let items: Vec<&OccRow> = occs.iter().filter(|o| f.items.contains(&o.0)).collect();
                let key = ("asm_folder", f.id);
                fold(ui, key, true, |ui| {
                    super::panels::tree_icon(ui, Icon::Project);
                    let r = ui.selectable_label(false, &f.name);
                    r.context_menu(|ui| {
                        let mut name = f.name.clone();
                        ui.horizontal(|ui| {
                            ui.label("Name");
                            if ui.text_edit_singleline(&mut name).changed() {
                                folder_cmd = Some(FolderCmd::Rename(f.id, name.clone()));
                            }
                        });
                        if ui
                            .button("Delete Folder")
                            .on_hover_text("The components stay")
                            .clicked()
                        {
                            folder_cmd = Some(FolderCmd::Delete(f.id));
                            ui.close();
                        }
                    });
                })
                .body(|ui| {
                    for o in items {
                        row(self, ui, o);
                    }
                });
            }
            for o in occs.iter().filter(|o| !in_folder.contains(&o.0)) {
                row(self, ui, o);
            }
            // Patterns after the components, their sources and copies
            // inside (Inventor's browser).
            for p in &patterns {
                self.pattern_rows(ui, p, &mut pcmds);
            }
            self.frame_rows(ui, &mut fcmds);
        });
        for c in cmds {
            match c {
                TreeCmd::Select(id, add) => {
                    if add {
                        if let Some(k) = self.asm.sel.iter().position(|&i| i == id) {
                            self.asm.sel.remove(k);
                        } else {
                            self.asm.sel.push(id);
                        }
                    } else {
                        self.asm.sel = vec![id];
                    }
                    self.cons.selected = None;
                }
                TreeCmd::SelectNested(occurrence, sub, add) => {
                    let s = super::asm_select::AsmSub::Part { occurrence, sub };
                    self.asm_sub_click(Some(s), add);
                }
                TreeCmd::OpenDoc(r) => self.open_doc(r),
                TreeCmd::Edit(top, sub) => self.edit_in_place(top, sub),
                TreeCmd::SelectFeature(occurrence, fid, add) => {
                    let sub = super::asm_select::AsmSub::Feature {
                        occurrence,
                        id: fid,
                        sub: Vec::new(),
                    };
                    self.asm_sub_click(Some(sub), add);
                }
                TreeCmd::OpenFeature(occurrence, k) => self.open_component_feature(occurrence, k),
                TreeCmd::FindFeature(occurrence, fid) => {
                    self.find_feature_in_window(occurrence, &fid)
                }
                TreeCmd::FeatureLook(occurrence, fid, look) => {
                    let t = [crate::assembly::looks::AsmLookTarget::Feature {
                        occurrence,
                        id: fid,
                    }];
                    self.asm_set_look(&t, look.as_deref());
                }
                TreeCmd::ToggleVisible(id) => self.toggle_visible(&[id]),
                TreeCmd::Pick(g) => {
                    if self.part_planned.is_some() {
                        self.planned_take_ref(g);
                    } else if self.cons.draft.is_some() {
                        self.constrain_pick(g);
                    } else {
                        self.file_status = Some(Ok(
                            "Constrain (C) takes origin and work geometry: open it, then click the row"
                                .into(),
                        ));
                    }
                }
                TreeCmd::SelectConstraint(id) => {
                    self.cons.selected = Some(id);
                    self.asm.sel.clear();
                }
                TreeCmd::EditConstraint(id) => self.edit_constraint(id),
                TreeCmd::SuppressConstraint(id) => self.toggle_constraint_suppress(id),
                TreeCmd::DeleteConstraint(_) if self.reps_refuse_structure() => {}
                TreeCmd::DeleteConstraint(id) => self.delete_constraint(id),
                TreeCmd::Drive(id) => self.open_drive(id),
            }
        }
        if let Some(c) = folder_cmd {
            self.folder_cmd(c);
        }
        for c in pcmds {
            self.pattern_cmd(c);
        }
        for c in fcmds {
            self.frame_cmd(c);
        }
    }

    fn folder_cmd(&mut self, c: FolderCmd) {
        match c {
            FolderCmd::Rename(id, name) => self.edit_assembly("Rename Folder", |a| {
                if let Some(f) = a.folders.iter_mut().find(|f| f.id == id) {
                    f.name = name;
                }
            }),
            FolderCmd::Delete(id) => self.edit_assembly("Delete Folder", |a| a.delete_folder(id)),
        }
    }

    /// The menu entries that file components into folders.
    pub(super) fn folder_menu(&mut self, ui: &mut egui::Ui, ids: &[u32]) {
        let folders: Vec<(u32, String)> = self
            .assembly()
            .map(|a| a.folders.iter().map(|f| (f.id, f.name.clone())).collect())
            .unwrap_or_default();
        if ui.button("Expand All Children").clicked() {
            super::panels::request_fold_all(ui.ctx(), true);
            ui.close();
        }
        if ui.button("Collapse All Children").clicked() {
            super::panels::request_fold_all(ui.ctx(), false);
            ui.close();
        }
        ui.menu_button("Add to Folder", |ui| {
            if ui.button("New Folder").clicked() {
                self.edit_assembly("Create Folder", |a| {
                    a.add_folder(None, ids);
                });
                ui.close();
            }
            for (id, name) in &folders {
                if ui.button(name).clicked() {
                    self.edit_assembly("Add to Folder", |a| a.move_to_folder(Some(*id), ids));
                    ui.close();
                }
            }
            if !folders.is_empty() && ui.button("Out of its folder").clicked() {
                self.edit_assembly("Remove from Folder", |a| a.move_to_folder(None, ids));
                ui.close();
            }
        });
    }

    /// Demote: the selected components into a new subassembly.
    pub(super) fn demote_selected(&mut self) {
        let ids = self.asm.sel.clone();
        if ids.is_empty() {
            self.file_status = Some(Err("Demote: select the components first".into()));
            return;
        }
        let doc = self.part_id;
        let name = self.project.free_name("SubAssembly");
        let mut copy = self.project.clone();
        match crate::assembly::restructure::demote(&mut copy, doc, &ids, &name) {
            Ok(d) => {
                self.checkpoint("Demote");
                self.project = copy;
                self.asm.sel = vec![d.occurrence];
                self.asm_solve(None);
                self.scene_dirty = true;
                self.file_status = Some(Ok(format!("Demote: {name} made")));
            }
            Err(e) => self.file_status = Some(Err(format!("Demote: {e}"))),
        }
    }

    /// Promote: the part selected inside a subassembly up to this assembly.
    pub(super) fn promote_selected(&mut self) {
        let picked = self.asm.sub.iter().find_map(|s| match s {
            super::asm_select::AsmSub::Part { occurrence, sub } if sub.len() == 1 => {
                Some((*occurrence, sub[0]))
            }
            _ => None,
        });
        let Some((top, inner)) = picked else {
            self.file_status = Some(Err(
                "Promote: select a component inside a subassembly in the browser".into(),
            ));
            return;
        };
        let doc = self.part_id;
        let mut copy = self.project.clone();
        match crate::assembly::restructure::promote(&mut copy, doc, top, inner) {
            Ok((id, removed)) => {
                self.checkpoint("Promote");
                self.project = copy;
                self.asm.sub.clear();
                self.asm.sel = vec![id];
                self.asm_solve(None);
                self.scene_dirty = true;
                self.file_status = Some(Ok(if removed.is_empty() {
                    "Promote: done".to_owned()
                } else {
                    format!(
                        "Promote: done; relationships removed: {}",
                        removed.join(", ")
                    )
                }));
            }
            Err(e) => self.file_status = Some(Err(format!("Promote: {e}"))),
        }
    }

    /// The components of subassembly `doc` below the node of occurrence
    /// `top` (`path`: the occurrences above them inside it): read-only rows,
    /// its relationships greyed in their folder; `stack` guards against an
    /// assembly that contains itself.
    fn nested_rows(
        &mut self,
        ui: &mut egui::Ui,
        top: u32,
        doc: crate::document::DocRef,
        path: &[u32],
        stack: &mut Vec<crate::document::DocumentId>,
        cmds: &mut Vec<TreeCmd>,
    ) {
        use crate::assembly::tree;
        if stack.contains(&doc.document) || stack.len() > tree::MAX_DEPTH {
            ui.label(RichText::new("(the assembly contains itself)").weak());
            return;
        }
        let linked = self.linked();
        let Some(a) = tree::assembly_of(&self.project, &linked, doc).cloned() else {
            return;
        };
        stack.push(doc.document);
        if !a.constraints.is_empty() {
            fold(ui, ("asm_sub_rel", top, path.to_vec()), false, |ui| {
                super::panels::tree_icon(ui, Icon::Coincident);
                ui.label(RichText::new("Relationships").weak());
            })
            .body(|ui| {
                let name = |r: &GeomRef| {
                    a.find(r.occurrence())
                        .map_or("Assembly".to_owned(), |o| o.name.clone())
                };
                for c in &a.constraints {
                    let text = format!("{} ({}, {})", c.name, name(&c.a), name(&c.b));
                    ui.horizontal(|ui| {
                        super::panels::tree_icon(ui, Icon::Coincident);
                        ui.label(RichText::new(text).weak()).on_hover_text(
                            "Relationships of the subassembly: Open it to edit them",
                        );
                    });
                }
            });
        }
        for o in &a.occurrences {
            let mut p = path.to_vec();
            p.push(o.id);
            // The component in the top file's terms (a linked file's own
            // references are relative to it).
            let component = tree::rebase(o.component, doc.library);
            let inner = tree::assembly_of(&self.project, &linked, component).is_some();
            let me = super::asm_select::AsmSub::Part {
                occurrence: top,
                sub: p.clone(),
            };
            let selected = self.asm.sub.contains(&me);
            let mut row = |ui: &mut egui::Ui, this: &mut Self| {
                // Visibility inside a subassembly is the subassembly's own.
                ui.add_enabled_ui(false, |ui| visibility_button(ui, o.visible));
                super::panels::tree_icon(ui, if inner { Icon::Assembly } else { Icon::Part });
                if o.grounded {
                    super::panels::tree_icon(ui, Icon::Ground);
                }
                // A component in a file of its own says so (one of this file does not).
                let linked_file = o.component.library.is_some();
                let mut text = RichText::new(o.name.clone());
                if !o.shown() {
                    text = text.weak();
                }
                let mut r = ui.selectable_label(selected, text);
                if linked_file {
                    r = r.on_hover_text(LINKED_FILE_TIP);
                }
                if r.clicked() {
                    let add = ui.input(|i| i.modifiers.command || i.modifiers.shift);
                    cmds.push(TreeCmd::SelectNested(top, p.clone(), add));
                }
                if r.double_clicked() {
                    cmds.push(TreeCmd::Edit(top, p.clone()));
                }
                if r.hovered() && !inner {
                    this.asm.tree_hover = Some(me.clone());
                }
                r.context_menu(|ui| {
                    if ui.button("Edit").clicked() {
                        cmds.push(TreeCmd::Edit(top, p.clone()));
                        ui.close();
                    }
                    if ui.button("Open").clicked() {
                        cmds.push(TreeCmd::OpenDoc(component));
                        ui.close();
                    }
                });
            };
            if inner {
                fold(ui, ("asm_sub_occ", top, p.clone()), false, |ui| {
                    row(ui, self)
                })
                .body(|ui| self.nested_rows(ui, top, component, &p, stack, cmds));
            } else {
                ui.horizontal(|ui| {
                    ui.add_space(18.0);
                    row(ui, self);
                });
            }
        }
        stack.pop();
    }

    /// The Origin folder of a component (or of the assembly).
    fn origin_folder(&mut self, ui: &mut egui::Ui, occurrence: u32, cmds: &mut Vec<TreeCmd>) {
        fold(ui, ("asm_origin", occurrence), false, |ui| {
            super::panels::tree_icon(ui, Icon::Origin);
            ui.label("Origin");
        })
        .body(|ui| {
            for item in OriginItem::ALL {
                let icon = match item {
                    OriginItem::XAxis | OriginItem::YAxis | OriginItem::ZAxis => Icon::WorkAxis,
                    OriginItem::Center => Icon::WorkPoint,
                    _ => Icon::WorkPlane,
                };
                let g = GeomRef::Origin {
                    sub: Vec::new(),
                    occurrence,
                    item,
                };
                self.geom_row(ui, icon, item.label(), g, cmds);
            }
        });
    }

    /// A row of origin or work geometry.
    fn geom_row(
        &mut self,
        ui: &mut egui::Ui,
        icon: Icon,
        label: &str,
        g: GeomRef,
        cmds: &mut Vec<TreeCmd>,
    ) {
        ui.horizontal(|ui| {
            super::panels::tree_icon(ui, icon);
            let picked = self.cons.draft.as_ref().is_some_and(|d| d.has(&g));
            let r = ui.selectable_label(picked, label);
            if r.hovered() {
                self.cons.browser_hover = Some(g.clone());
            }
            if r.clicked() {
                cmds.push(TreeCmd::Pick(g));
            }
        });
    }

    /// The browser's bar: Assembly / Modeling View, Display Preferences
    /// (filters) and the search field.
    fn asm_browser_bar(&mut self, ui: &mut egui::Ui) {
        let o = &mut self.asm_browser;
        ui.horizontal(|ui| {
            if ui
                .selectable_label(!o.modeling, "Assembly View")
                .on_hover_text("Relationships under the components they engage")
                .clicked()
            {
                o.modeling = false;
            }
            if ui
                .selectable_label(o.modeling, "Modeling View")
                .on_hover_text("The parts' features under the components, as in the part")
                .clicked()
            {
                o.modeling = true;
            }
            if super::theme::glyph(ui, super::theme::Glyph::Plus)
                .on_hover_text("Expand All Children")
                .clicked()
            {
                super::panels::request_fold_all(ui.ctx(), true);
            }
            if super::theme::glyph(ui, super::theme::Glyph::Minus)
                .on_hover_text("Collapse All Children")
                .clicked()
            {
                super::panels::request_fold_all(ui.ctx(), false);
            }
            ui.menu_button("Filter", |ui| {
                ui.checkbox(&mut o.hide_work, "Hide Work Features")
                    .on_hover_text("Work features and the Origin folders");
                ui.checkbox(&mut o.hide_fully_constrained, "Hide Fully Constrained")
                    .on_hover_text("Only components that can still move");
                ui.checkbox(&mut o.extended_names, "Show Extended Names")
                    .on_hover_text("A feature's operation and main value (Modeling View)");
                ui.checkbox(&mut o.constraint_state, "Show Constraint State")
                    .on_hover_text(
                        "A filled dot: fully constrained; hollow: degrees of freedom left",
                    );
            });
        });
        ui.horizontal(|ui| {
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.asm.search)
                    .hint_text("Search")
                    .desired_width(ui.available_width() - 28.0),
            );
            if std::mem::take(&mut self.asm.search_focus) {
                r.request_focus();
            }
            if r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.asm.search.clear();
            }
            if ui
                .add_enabled(
                    !self.asm.search.is_empty(),
                    super::theme::GlyphButton(super::theme::Glyph::Close),
                )
                .on_hover_text("Clear the search")
                .clicked()
            {
                self.asm.search.clear();
            }
        });
    }

    /// A component's degrees of freedom left (0: grounded, a pattern copy,
    /// or fully constrained).
    fn occurrence_dof(&self, id: u32) -> usize {
        let Some(a) = self.assembly() else {
            return 0;
        };
        if a.find(id).is_some_and(|o| o.grounded) || a.is_element(id) {
            return 0;
        }
        self.cons.last.dof.get(&id).copied().unwrap_or(6)
    }

    /// A feature of a component's part (Modeling View), with the sketches it
    /// consumed inside.
    fn feature_row(
        &mut self,
        ui: &mut egui::Ui,
        occurrence: u32,
        f: &FeatureRow,
        extended: bool,
        cmds: &mut Vec<TreeCmd>,
    ) {
        let sub = super::asm_select::AsmSub::Feature {
            occurrence,
            id: f.id.clone(),
            sub: Vec::new(),
        };
        let selected = self.asm.sub.contains(&sub);
        let mut header = |ui: &mut egui::Ui| {
            super::panels::tree_icon(ui, f.icon);
            let mut text = RichText::new(if extended {
                format!("{} ({})", f.name, f.about)
            } else {
                f.name.clone()
            });
            if f.suppressed {
                text = text.weak().strikethrough();
            }
            let r = ui.selectable_label(selected, text).on_hover_text(&f.about);
            if r.hovered() {
                self.asm.tree_hover = Some(sub.clone());
            }
            if r.clicked() {
                let add = ui.input(|i| i.modifiers.command || i.modifiers.shift);
                cmds.push(TreeCmd::SelectFeature(occurrence, f.id.clone(), add));
            }
            if r.double_clicked() {
                cmds.push(TreeCmd::OpenFeature(occurrence, f.k));
            }
            r.context_menu(|ui| {
                if ui
                    .button("Open")
                    .on_hover_text("Open the part with this feature selected")
                    .clicked()
                {
                    cmds.push(TreeCmd::OpenFeature(occurrence, f.k));
                    ui.close();
                }
                if ui
                    .button("Find in Window")
                    .on_hover_text("Zoom to the feature")
                    .clicked()
                {
                    cmds.push(TreeCmd::FindFeature(occurrence, f.id.clone()));
                    ui.close();
                }
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
                    cmds.push(TreeCmd::FeatureLook(occurrence, f.id.clone(), n));
                    ui.close();
                }
            });
        };
        if f.sketches.is_empty() {
            ui.horizontal(|ui| {
                // Line up with the rows that fold.
                ui.add_space(ui.spacing().indent);
                header(ui);
            });
            return;
        }
        fold(ui, ("asm_feature", occurrence, &f.id), false, header).body(|ui| {
            for name in &f.sketches {
                ui.horizontal(|ui| {
                    super::panels::tree_icon(ui, Icon::Sketch);
                    ui.label(RichText::new(name).weak())
                        .on_hover_text("Sketches are not shown in an assembly");
                });
            }
        });
    }

    /// One constraint (red when sick, grey when suppressed).
    fn constraint_row(&mut self, ui: &mut egui::Ui, id: u32, cmds: &mut Vec<TreeCmd>) {
        let linked = self.linked();
        let Some(a) = self.assembly() else {
            return;
        };
        let Some(c) = a.constraint(id) else {
            return;
        };
        let between = |r: &GeomRef| {
            if r.occurrence() == ASSEMBLY {
                "Assembly".to_owned()
            } else {
                // A part inside a subassembly: `Bracket:1/Post:2`.
                crate::assembly::tree::nested(&self.project, &linked, a, r.occurrence(), r.sub())
                    .map_or("?".to_owned(), |n| n.name)
            }
        };
        let mut label = format!("{} ({}, {})", c.name, between(&c.a), between(&c.b));
        let suppressed = c.suppressed;
        // Overridden in the active positional representation: bold, with the value.
        let overridden = self.rel_override_text(id);
        if let Some(t) = &overridden {
            label = format!("{label} {t}");
        }
        let sick = self
            .cons
            .last
            .sick
            .iter()
            .find(|(i, _)| *i == id)
            .map(|(_, e)| e.clone());
        ui.horizontal(|ui| {
            super::panels::tree_icon(ui, Icon::Coincident);
            let mut text = RichText::new(label);
            if overridden.is_some() {
                text = text.strong();
            }
            if suppressed {
                text = text.weak();
            } else if sick.is_some() {
                text = text.color(super::theme::tokens().error);
            }
            let r = ui.selectable_label(self.cons.selected == Some(id), text);
            let r = match &sick {
                Some(e) => r.on_hover_text(format!("Sick: {e}")),
                None => r,
            };
            if r.clicked() {
                cmds.push(TreeCmd::SelectConstraint(id));
            }
            if r.double_clicked() {
                cmds.push(TreeCmd::EditConstraint(id));
            }
            r.context_menu(|ui| {
                if ui.button("Edit").clicked() {
                    cmds.push(TreeCmd::EditConstraint(id));
                    ui.close();
                }
                if ui
                    .button("Drive...")
                    .on_hover_text("Animate its value")
                    .clicked()
                {
                    cmds.push(TreeCmd::Drive(id));
                    ui.close();
                }
                let s = if suppressed { "Unsuppress" } else { "Suppress" };
                if ui.button(s).clicked() {
                    cmds.push(TreeCmd::SuppressConstraint(id));
                    ui.close();
                }
                self.override_menu(ui, super::reps::OverrideOf::Relationship(id));
                if ui.button("Delete").clicked() {
                    cmds.push(TreeCmd::DeleteConstraint(id));
                    ui.close();
                }
            });
        });
    }
}

/// A node that folds open and shut (its state remembered by egui).
pub(super) fn fold<'a>(
    ui: &'a mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    open: bool,
    header: impl FnOnce(&mut egui::Ui),
) -> egui::collapsing_header::HeaderResponse<'a, ()> {
    super::panels::tree_fold(ui, ui.make_persistent_id(id), open).show_header(ui, header)
}

/// The eye at the start of a component row: open = visible. Returns whether
/// it was clicked.
fn visibility_button(ui: &mut egui::Ui, visible: bool) -> bool {
    let (r, resp) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::click());
    let palette = if visible {
        super::icons::Palette::enabled()
    } else {
        super::icons::Palette::disabled()
    };
    super::icons::paint(ui.painter(), r, Icon::AnalysisVisibility, &palette);
    if !visible {
        let c = ui.visuals().weak_text_color();
        ui.painter()
            .line_segment([r.left_bottom(), r.right_top()], egui::Stroke::new(1.5, c));
    }
    resp.on_hover_text(if visible {
        "Visible: click to hide"
    } else {
        "Hidden: click to show"
    })
    .clicked()
}

/// Show Constraint State: filled = no degrees of freedom left, hollow =
/// some left.
fn state_dot(ui: &mut egui::Ui, dof: usize) {
    let (r, resp) = ui.allocate_exact_size(egui::vec2(10.0, 16.0), egui::Sense::hover());
    let c = ui.visuals().text_color();
    if dof == 0 {
        ui.painter().circle_filled(r.center(), 3.5, c);
    } else {
        ui.painter()
            .circle_stroke(r.center(), 3.5, egui::Stroke::new(1.2, c));
    }
    resp.on_hover_text(if dof == 0 {
        "Fully constrained".to_owned()
    } else {
        format!("{dof} degree(s) of freedom left")
    });
}
