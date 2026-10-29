//! Side panels: the Model Browser (left), the Properties panel (right), the
//! status bar and the Application Options window.

use eframe::egui::{self, Color32, RichText};

use super::browser_edit::{BrowserDrag, RenameTarget};
use super::icons::Icon;
use super::{CadCraftApp, Mode, MousePreset, PickPurpose, Selection};
use crate::document::DocumentData;
use crate::part::{Feature, OriginPlane, PlaneRef, PointRef};
use crate::sketch::{Constraint, SolveStatus};

/// Browser actions, collected while drawing and applied afterwards.
enum BrowserCmd {
    Select(Selection),
    /// The iProperties window of a document.
    IProperties(crate::document::DocumentId),
    /// A Manage › Documents command on a document (spec 0.6b).
    Doc(super::documents::DocCmd, crate::document::DocumentId),
    NewSketchOn(PlaneRef),
    Edit(usize),
    Finish,
    Redefine(usize),
    LookAt(usize),
    SetVisible(usize, bool),
    Delete(usize),
    DeleteFeature(usize),
    /// Edit Feature: the feature's dialog with its inputs, the part rolled
    /// back to it.
    EditFeature(usize),
    /// Share (true) or unshare a sketch.
    Share(usize, bool),
    /// Show / hide a feature (work planes).
    FeatureVisible(usize, bool),
    /// Ground a work point where it is now (Inventor's Ground).
    GroundPoint(usize),
    /// Show Inputs: select what a work feature is built from.
    ShowInputs(usize),
    /// Appearance → … on a feature or body (`None`: remove the override).
    Look(crate::part::appearance::LookTarget, Option<String>),
    /// Material → … on a body.
    BodyMaterial(String, Option<String>),
    /// Visibility of a body (persistent name).
    BodyVisible(String, bool),
    /// Suppress (true) / Unsuppress Features.
    Suppress(usize, bool),
    /// Suppress if... (the condition window for feature k).
    SuppressIf(usize),
    /// Move the End of Part so that this many features are built.
    MoveEop(usize),
    /// Delete All Features Below EOP.
    DeleteBelowEop,
    /// Reorder: feature `from` to index `to`.
    MoveFeature(usize, usize),
    /// Start renaming a row (F2, slow double-click, Rename).
    StartRename(RenameTarget),
    /// The rename field was confirmed.
    Rename(RenameTarget, String),
    /// The rename field was left with Esc.
    CancelRename,
    /// The pointer is over this feature (by id): highlight it in the view.
    Hover(String),
    /// A row clicked while Combine / Split is open: an input for its active
    /// selector (an origin plane, a body or a surface).
    CommandPick(super::combine_split::BodyPick),
}

const WARNING: Color32 = Color32::from_rgb(230, 140, 0);

/// Time after a click on a selected row before it turns into a rename
/// field (a second click sooner is a double-click: Edit).
const SLOW_CLICK_S: f64 = 0.5;

/// A 16 px browser icon before the next widget in a horizontal row.
pub(super) fn tree_icon(ui: &mut egui::Ui, icon: Icon) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
    super::icons::paint(ui.painter(), r, icon, &super::icons::Palette::ENABLED);
}

/// The pending slow-click rename: (row, when it starts).
fn pending_rename_id() -> egui::Id {
    egui::Id::new("browser_pending_rename")
}

/// A click on `selected` row `target`: a slow second click starts a
/// rename after [`SLOW_CLICK_S`] unless a double-click comes first.
fn slow_click(ui: &egui::Ui, r: &egui::Response, selected: bool, target: RenameTarget) {
    let now = ui.input(|i| i.time);
    let key = egui::Id::new(("browser_click", format!("{target:?}")));
    let last: Option<f64> = ui.data(|d| d.get_temp(key));
    ui.data_mut(|d| d.insert_temp(key, now));
    if r.double_clicked() {
        ui.data_mut(|d| d.remove::<(RenameTarget, f64)>(pending_rename_id()));
    } else if r.clicked() && selected && last.is_none_or(|t| now - t > SLOW_CLICK_S) {
        ui.data_mut(|d| d.insert_temp(pending_rename_id(), (target, now + SLOW_CLICK_S)));
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(SLOW_CLICK_S));
    }
}

/// The rename field of a row (Enter / click elsewhere: apply; Esc: cancel).
fn rename_field(ui: &mut egui::Ui, target: RenameTarget, cmds: &mut Vec<BrowserCmd>) {
    let id = super::browser_edit::rename_buffer_id();
    let (mut text, focused): (String, bool) = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(150.0));
    if !focused {
        // Once only: re-requesting every frame swallows Enter.
        super::valuefield::focus_all(ui.ctx(), &r, &text);
    }
    ui.data_mut(|d| d.insert_temp(id, (text.clone(), true)));
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        cmds.push(BrowserCmd::CancelRename);
    } else if r.lost_focus() || (focused && !r.has_focus()) {
        cmds.push(BrowserCmd::Rename(target, text));
    }
}

/// Drag and drop on a feature row `k`: the row can be dragged (reorder),
/// and a feature or the End of Part dropped on it goes above (upper half)
/// or below it.
fn drop_target(ui: &egui::Ui, r: &egui::Response, k: usize, cmds: &mut Vec<BrowserCmd>) {
    r.dnd_set_drag_payload(BrowserDrag::Feature(k));
    let below = ui
        .ctx()
        .pointer_interact_pos()
        .is_some_and(|p| p.y > r.rect.center().y);
    if r.dnd_hover_payload::<BrowserDrag>().is_some() {
        let y = if below { r.rect.bottom() } else { r.rect.top() };
        let x = ui.max_rect().x_range();
        ui.painter().hline(
            x,
            y,
            egui::Stroke::new(2.0, ui.visuals().selection.stroke.color),
        );
    }
    if let Some(p) = r.dnd_release_payload::<BrowserDrag>() {
        let gap = if below { k + 1 } else { k };
        cmds.push(match *p {
            BrowserDrag::Feature(from) => {
                BrowserCmd::MoveFeature(from, if gap > from { gap - 1 } else { gap })
            }
            BrowserDrag::Eop => BrowserCmd::MoveEop(gap),
        });
    }
}

impl CadCraftApp {
    /// Model Browser: project → documents → Origin, features, sketches.
    pub(super) fn browser(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Model").strong());
        ui.separator();
        self.browser_hover = None;
        let mut cmds = Vec::new();
        let mut activate = None;
        let linked_back = self
            .in_place_now()
            .and_then(|ip| ip.linked.as_ref())
            .map(|le| le.assembly_name.clone());
        if let Some(back) = linked_back.or_else(|| {
            self.asm
                .came_from
                .and_then(|id| self.project.document(id))
                .map(|d| d.name.clone())
        }) && ui
            .button(format!("Return to {back}"))
            .on_hover_text("Back to the assembly this part was opened from")
            .clicked()
        {
            self.return_to_assembly();
            return;
        }
        let docs: Vec<(
            crate::document::DocumentId,
            String,
            crate::document::DocumentKind,
        )> = self
            .project
            .documents()
            .iter()
            .map(|d| (d.id, d.name.clone(), d.data.kind()))
            .collect();
        egui::ScrollArea::vertical().show(ui, |ui| {
            let project_label = RichText::new(self.project.name.clone()).strong();
            ui.horizontal(|ui| {
                tree_icon(ui, Icon::Project);
                let r = ui.selectable_label(self.selection == Selection::Project, project_label);
                if r.clicked() {
                    cmds.push(BrowserCmd::Select(Selection::Project));
                }
                // New documents in this file (spec 0.6b §3.2).
                r.context_menu(|ui| {
                    let mut ask = None;
                    super::documents::new_in_file_menu(ui, &mut ask);
                    if let Some(c) = ask {
                        cmds.push(BrowserCmd::Doc(c, self.part_id));
                    }
                });
            });
            let only_active = self.is_assembly();
            ui.indent("docs", |ui| {
                for (id, name, kind) in &docs {
                    // An assembly's browser shows its own tree only; the
                    // other documents are in Manage > Documents and the tabs.
                    if only_active && *id != self.part_id {
                        continue;
                    }
                    if *id == self.part_id {
                        if self.is_drawing() {
                            self.drawing_tree(ui, name);
                        } else if self.is_assembly() {
                            self.assembly_tree(ui, name);
                        } else {
                            self.part_tree(ui, name, &mut cmds);
                        }
                    } else {
                        ui.horizontal(|ui| {
                            tree_icon(
                                ui,
                                match kind {
                                    crate::document::DocumentKind::Assembly => Icon::Assembly,
                                    crate::document::DocumentKind::Drawing => Icon::Drawing,
                                    crate::document::DocumentKind::Part => Icon::Part,
                                },
                            );
                            let r = ui.selectable_label(
                                self.selection == Selection::Document(*id),
                                name.clone(),
                            );
                            let r = r.on_hover_text(
                                "Double-click to open (make it the active document)",
                            );
                            if r.double_clicked() {
                                activate = Some(*id);
                            } else if r.clicked() {
                                cmds.push(BrowserCmd::Select(Selection::Document(*id)));
                            }
                            r.context_menu(|ui| {
                                let mut ask = None;
                                super::documents::doc_menu(ui, *id, &mut ask, &mut activate);
                                if let Some((c, d)) = ask {
                                    cmds.push(BrowserCmd::Doc(c, d));
                                }
                                if ui.button("iProperties...").clicked() {
                                    cmds.push(BrowserCmd::IProperties(*id));
                                    ui.close();
                                }
                            });
                        });
                    }
                }
            });
        });
        let ctx = ui.ctx().clone();
        // F2 over the browser renames the selected row (in the view F2 is
        // Pan, as in Inventor).
        let over = ui.rect_contains_pointer(ui.max_rect());
        if over && self.rename.is_none() && ctx.input(|i| i.key_pressed(egui::Key::F2)) {
            let part = self.part();
            let target = match self.selection {
                Selection::Feature(k) => part
                    .features
                    .get(k)
                    .map(|f| RenameTarget::Feature(f.id().to_owned())),
                Selection::Sketch(i) => part
                    .sketches
                    .get(i)
                    .map(|s| RenameTarget::Sketch(s.id.clone())),
                _ => None,
            };
            if let Some(t) = target {
                cmds.push(BrowserCmd::StartRename(t));
            }
        }
        // A slow second click that was not a double-click.
        let pending: Option<(RenameTarget, f64)> = ctx.data(|d| d.get_temp(pending_rename_id()));
        if let Some((t, due)) = pending
            && ctx.input(|i| i.time) >= due
        {
            ctx.data_mut(|d| d.remove::<(RenameTarget, f64)>(pending_rename_id()));
            cmds.push(BrowserCmd::StartRename(t));
        }
        // Esc aborts a drag (Inventor: aborts moving the EOP).
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            egui::DragAndDrop::clear_payload(&ctx);
        }
        for cmd in cmds {
            match cmd {
                BrowserCmd::StartRename(t) => self.start_rename(&ctx, t),
                c => self.browser_cmd(c),
            }
        }
        if let Some(id) = activate {
            self.asm.came_from = None;
            self.activate_document(id);
        }
    }

    fn browser_cmd(&mut self, cmd: BrowserCmd) {
        match cmd {
            BrowserCmd::IProperties(d) => self.open_iproperties(d),
            BrowserCmd::Doc(c, d) => self.docs_apply_to(c, d),
            // The pattern command's Features selector takes browser rows.
            BrowserCmd::Select(Selection::Feature(k))
                if self.pattern_draft.is_some() && self.pattern_pick_browser(k) => {}
            // Thicken's Quilts / Solids selectors take body rows too.
            BrowserCmd::CommandPick(
                super::combine_split::BodyPick::Surface(n)
                | super::combine_split::BodyPick::Body(n),
            ) if self.thicken_draft.is_some() => self.thicken_pick_body(n),
            BrowserCmd::CommandPick(
                super::combine_split::BodyPick::Surface(n)
                | super::combine_split::BodyPick::Body(n),
            ) if self.move_draft.is_some() => self.move_bodies_pick_body(n),
            // A planned panel (Copy Object, the surface commands) takes body rows too.
            BrowserCmd::CommandPick(
                super::combine_split::BodyPick::Surface(n)
                | super::combine_split::BodyPick::Body(n),
            ) if self.part_planned.is_some() => self.part_planned_pick_body(n),
            BrowserCmd::CommandPick(p) => {
                let row = match &p {
                    super::combine_split::BodyPick::Surface(n)
                    | super::combine_split::BodyPick::Body(n) => Some(n.clone()),
                    _ => None,
                };
                // No command takes it: the body is selected (Visibility, Alt+V, its menu),
                // hidden ones too.
                if !self.body_cmd_browser_pick(p)
                    && let Some(n) = row
                    && self.editing.is_none()
                {
                    self.sel = vec![super::select::SelItem::Body(n)];
                    self.face_sel = None;
                    self.edge_sel = None;
                }
            }
            // Split's tool from the browser: a work plane or a sketch row.
            BrowserCmd::Select(Selection::Feature(k))
                if self.split_draft.is_some()
                    && matches!(
                        self.part().features.get(k),
                        Some(Feature::Work(w)) if matches!(w.def, crate::part::WorkDef::Plane(_))
                    )
                    && self.body_cmd_browser_pick(super::combine_split::BodyPick::Plane(
                        PlaneRef::Work(self.part().features[k].id().to_owned()),
                    )) => {}
            BrowserCmd::Select(Selection::Sketch(i))
                if self.split_draft.is_some()
                    && i < self.part().sketches.len()
                    && self.body_cmd_browser_pick(super::combine_split::BodyPick::Sketch(
                        self.part().sketches[i].id.clone(),
                    )) => {}
            BrowserCmd::Select(s) => {
                // The feature's faces are highlighted in the view.
                if let Selection::Feature(k) = s
                    && let Some(f) = self.part().features.get(k)
                {
                    self.sel = vec![super::select::SelItem::Feature(f.id().to_owned())];
                }
                self.selection = s;
            }
            BrowserCmd::NewSketchOn(plane) => {
                self.finish_sketch();
                self.mode = Mode::PickPlane(PickPurpose::NewSketch);
                self.plane_picked(plane);
            }
            BrowserCmd::Edit(i) => self.edit_sketch(i),
            BrowserCmd::Finish => self.finish_sketch(),
            BrowserCmd::Redefine(i) => {
                self.finish_sketch();
                self.selection = Selection::Sketch(i);
                self.mode = Mode::PickPlane(PickPurpose::Redefine(i));
            }
            BrowserCmd::LookAt(i) => self.look_at_sketch(i),
            BrowserCmd::SetVisible(i, v) => {
                self.checkpoint("Visibility");
                self.part_mut().sketches[i].visible = v;
            }
            BrowserCmd::Delete(i) => self.delete_sketch(i),
            BrowserCmd::DeleteFeature(k) => self.delete_feature(k),
            BrowserCmd::EditFeature(k) => self.edit_feature_dialog(k),
            BrowserCmd::FeatureVisible(k, v) => {
                self.checkpoint("Visibility");
                if let Some(Feature::Work(w)) = self.part_mut().features.get_mut(k) {
                    w.visible = v;
                }
            }
            BrowserCmd::ShowInputs(k) => {
                let items = self.work_inputs(k);
                if items.is_empty() {
                    self.file_status = Some(Ok(
                        "Show Inputs: built from the origin or fixed in space".into(),
                    ));
                } else {
                    self.sel = items;
                    self.scene_dirty = true;
                }
            }
            BrowserCmd::GroundPoint(k) => {
                let Some(Feature::Work(w)) = self.part().features.get(k).cloned() else {
                    return;
                };
                let Some(p) = self.part().work.points.get(&w.id).copied() else {
                    self.file_status = Some(Err("Ground: the point has no position".into()));
                    return;
                };
                let mut g = w.clone();
                g.def = crate::part::WorkDef::Point(crate::part::WorkPointDef::Fixed {
                    x: p.x,
                    y: p.y,
                    z: p.z,
                });
                self.edit_feature(k, Feature::Work(g));
            }
            BrowserCmd::Look(t, a) => self.assign_appearance(&[t], a.as_deref()),
            BrowserCmd::BodyMaterial(b, m) => self.assign_material(&[b], m.as_deref()),
            BrowserCmd::BodyVisible(b, v) => self.set_body_visible(&b, v),
            BrowserCmd::Share(i, on) => {
                self.checkpoint(if on { "Share Sketch" } else { "Unshare Sketch" });
                let part = self.part_mut();
                let r = if on {
                    part.share_sketch(i)
                } else {
                    part.unshare_sketch(i)
                };
                if let Err(e) = r {
                    self.history.cancel();
                    self.file_status = Some(Err(e.to_string()));
                }
            }
            BrowserCmd::Suppress(k, on) => self.set_suppressed(k, on),
            BrowserCmd::SuppressIf(k) => {
                let id = self.part().features.get(k).map(|f| f.id().to_owned());
                let text = id
                    .and_then(|id| self.part().suppress_if.get(&id).cloned())
                    .unwrap_or_default();
                self.suppress_if_edit =
                    Some((k, super::if_builder::CondBuilder::from_text(&text), None));
            }
            BrowserCmd::MoveEop(at) => self.move_eop(at),
            BrowserCmd::DeleteBelowEop => self.delete_below_eop(),
            BrowserCmd::MoveFeature(from, to) => self.move_feature(from, to),
            // Needs the context: handled in `browser`.
            BrowserCmd::StartRename(_) => {}
            BrowserCmd::Rename(t, name) => self.apply_rename(t, name),
            BrowserCmd::CancelRename => self.rename = None,
            BrowserCmd::Hover(id) => self.browser_hover = Some(id),
        }
    }

    /// The tree of the active part.
    fn part_tree(&self, ui: &mut egui::Ui, name: &str, cmds: &mut Vec<BrowserCmd>) {
        let part = self.part();
        egui::collapsing_header::CollapsingState::load_with_default_open(
            ui.ctx(),
            ui.make_persistent_id("active_part"),
            true,
        )
        .show_header(ui, |ui| {
            tree_icon(ui, Icon::Part);
            ui.label(RichText::new(name).strong()).context_menu(|ui| {
                if ui.button("iProperties...").clicked() {
                    cmds.push(BrowserCmd::IProperties(self.part_id));
                    ui.close();
                }
            });
        })
        .body(|ui| {
            let bodies = part.body.solids().filter(|(_, s)| !s.open).count();
            // One row per surface body name: a body in pieces apart (stitched strips meeting at a
            // point) is several solids of the B-rep with one name.
            let surfaces = {
                let mut names: Vec<Option<&String>> = part
                    .body
                    .solids()
                    .filter(|(_, s)| s.open)
                    .map(|(_, s)| s.name.as_ref())
                    .collect();
                names.sort();
                names.dedup_by(|a, b| a.is_some() && a == b);
                names.len()
            };
            egui::collapsing_header::CollapsingState::load_with_default_open(
                ui.ctx(),
                ui.make_persistent_id("solid_bodies"),
                false,
            )
            .show_header(ui, |ui| {
                tree_icon(ui, Icon::Box);
                ui.label(format!("Solid Bodies({bodies})"));
            })
            .body(|ui| {
                // Display names by the creating feature (Solid1, Solid2...).
                let names = part.bodies_before(usize::MAX);
                let mut labels: Vec<(String, String)> = part
                    .body
                    .solids()
                    .filter(|(_, s)| !s.open)
                    .map(|(_, s)| {
                        let id = s.name.clone().unwrap_or_default();
                        let label = names
                            .iter()
                            .find(|(n, _)| *n == id)
                            .map_or_else(|| "Solid".to_owned(), |(_, l)| l.clone());
                        (label, id)
                    })
                    .collect();
                labels.sort();
                labels.dedup();
                for (label, id) in labels {
                    ui.horizontal(|ui| {
                        tree_icon(ui, Icon::Part);
                        let shown = part.body_visible(&id);
                        let text = if shown {
                            RichText::new(label)
                        } else {
                            RichText::new(label).weak()
                        };
                        let r = ui
                            .selectable_label(
                                self.sel.contains(&super::select::SelItem::Body(id.clone())),
                                text,
                            )
                            .on_hover_text(format!("Created by {id}"));
                        if r.clicked() {
                            cmds.push(BrowserCmd::Select(Selection::Body));
                            cmds.push(BrowserCmd::CommandPick(
                                super::combine_split::BodyPick::Body(id.clone()),
                            ));
                        }
                        r.context_menu(|ui| {
                            let mut v = shown;
                            if ui.checkbox(&mut v, "Visibility").clicked() {
                                cmds.push(BrowserCmd::BodyVisible(id.clone(), v));
                                ui.close();
                            }
                            if let Some(m) = self.material_submenu(ui) {
                                cmds.push(BrowserCmd::BodyMaterial(id.clone(), m));
                                ui.close();
                            }
                            if let Some(a) = self.appearance_submenu(ui) {
                                use crate::part::appearance::LookTarget;
                                cmds.push(BrowserCmd::Look(LookTarget::Body(id.clone()), a));
                                ui.close();
                            }
                        });
                    });
                }
            });
            if surfaces > 0 {
                egui::collapsing_header::CollapsingState::load_with_default_open(
                    ui.ctx(),
                    ui.make_persistent_id("surface_bodies"),
                    true,
                )
                .show_header(ui, |ui| {
                    tree_icon(ui, Icon::Part);
                    ui.label(format!("Surface Bodies({surfaces})"));
                })
                .body(|ui| {
                    let mut ids: Vec<String> = part
                        .body
                        .solids()
                        .filter(|(_, s)| s.open)
                        .filter_map(|(_, s)| s.name.clone())
                        .collect();
                    ids.sort();
                    ids.dedup();
                    for id in ids {
                        ui.horizontal(|ui| {
                            tree_icon(ui, Icon::Part);
                            let shown = part.body_visible(&id);
                            let label = part
                                .features
                                .iter()
                                .find(|f| f.id() == id.split('#').next().unwrap_or(&id))
                                .map_or_else(|| id.clone(), |f| format!("Srf: {}", f.name()));
                            let text = if shown {
                                RichText::new(label)
                            } else {
                                RichText::new(label).weak()
                            };
                            let r = ui
                                .selectable_label(
                                    self.sel.contains(&super::select::SelItem::Body(id.clone())),
                                    text,
                                )
                                .on_hover_text(format!("Surface body {id} (a Split tool)"));
                            if r.clicked() {
                                cmds.push(BrowserCmd::CommandPick(
                                    super::combine_split::BodyPick::Surface(id.clone()),
                                ));
                            }
                            r.context_menu(|ui| {
                                let mut v = shown;
                                if ui.checkbox(&mut v, "Visibility").clicked() {
                                    cmds.push(BrowserCmd::BodyVisible(id.clone(), v));
                                    ui.close();
                                }
                            });
                        });
                    }
                });
            }
            egui::collapsing_header::CollapsingState::load_with_default_open(
                ui.ctx(),
                ui.make_persistent_id("origin"),
                false,
            )
            .show_header(ui, |ui| {
                tree_icon(ui, Icon::Origin);
                ui.label("Origin");
            })
            .body(|ui| {
                for p in OriginPlane::ALL {
                    ui.horizontal(|ui| {
                        tree_icon(ui, Icon::WorkPlane);
                        let r = ui
                            .selectable_label(false, p.label())
                            .on_hover_text("Right-click: New Sketch");
                        if r.clicked() {
                            cmds.push(BrowserCmd::CommandPick(
                                super::combine_split::BodyPick::Plane(PlaneRef::Origin(p)),
                            ));
                        }
                        r.context_menu(|ui| {
                            if ui.button("New Sketch").clicked() {
                                cmds.push(BrowserCmd::NewSketchOn(PlaneRef::Origin(p)));
                                ui.close();
                            }
                        });
                    });
                }
                for (icon, item) in [
                    (Icon::WorkAxis, "X Axis"),
                    (Icon::WorkAxis, "Y Axis"),
                    (Icon::WorkAxis, "Z Axis"),
                    (Icon::WorkPoint, "Center Point"),
                ] {
                    ui.horizontal(|ui| {
                        tree_icon(ui, icon);
                        ui.label(RichText::new(item).weak());
                    });
                }
            });

            // History order (Inventor): sketches and features as they were
            // created. A consumed sketch is nested under its feature; a
            // shared one also stands above its first feature; a free
            // sketch stays where it was created. The End of Part marker
            // stands where the history stops; below it everything is
            // rolled back (greyed).
            let mut listed = vec![false; part.sketches.len()];
            let n = part.features.len();
            let eop = part.eop_index();
            for pos in 0..=n {
                for (i, ps) in part.sketches.iter().enumerate() {
                    let at = ps.after_features.unwrap_or(n).min(n);
                    if !listed[i] && at == pos && part.consumers(&ps.id).is_empty() {
                        listed[i] = true;
                        self.sketch_row(ui, i, cmds);
                    }
                }
                if pos == eop {
                    self.eop_row(ui, cmds);
                }
                if let Some(f) = part.features.get(pos) {
                    self.feature_node(ui, pos, f, &mut listed, cmds);
                }
            }
            for (i, done) in listed.iter().enumerate() {
                if !done {
                    self.sketch_row(ui, i, cmds);
                }
            }
        });
    }

    /// The End of Part row: drag it onto a feature (above / below it), or
    /// Move EOP to Top / End from its menu.
    fn eop_row(&self, ui: &mut egui::Ui, cmds: &mut Vec<BrowserCmd>) {
        let part = self.part();
        let n = part.features.len();
        let eop = part.eop_index();
        ui.horizontal(|ui| {
            let (bar, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
            ui.painter().rect_filled(
                egui::Rect::from_center_size(bar.center(), egui::vec2(12.0, 4.0)),
                1.0,
                Color32::from_rgb(200, 60, 60),
            );
            let text = RichText::new("End of Part").italics();
            let r = ui
                .add(egui::Label::new(text).sense(egui::Sense::click_and_drag()))
                .on_hover_text(
                    "Drag onto a feature: the features below are rolled back (not built), \
                     new features are inserted here. Esc aborts the drag. \
                     Right-click: Move EOP to Top / End",
                );
            r.dnd_set_drag_payload(BrowserDrag::Eop);
            if r.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            }
            r.context_menu(|ui| {
                if ui
                    .add_enabled(eop > 0, egui::Button::new("Move EOP to Top"))
                    .clicked()
                {
                    cmds.push(BrowserCmd::MoveEop(0));
                    ui.close();
                }
                if ui
                    .add_enabled(eop < n, egui::Button::new("Move EOP to End"))
                    .clicked()
                {
                    cmds.push(BrowserCmd::MoveEop(n));
                    ui.close();
                }
                ui.separator();
                if ui
                    .add_enabled(eop < n, egui::Button::new("Delete All Features Below EOP"))
                    .clicked()
                {
                    cmds.push(BrowserCmd::DeleteBelowEop);
                    ui.close();
                }
            });
        });
    }

    /// The name of feature `k` in its row: icon, text (orange when sick,
    /// grey when suppressed or below the End of Part), hover text, click /
    /// double-click / slow click (rename), drag and drop. `None` while the
    /// row is being renamed.
    #[allow(clippy::too_many_arguments)]
    fn feature_label(
        &self,
        ui: &mut egui::Ui,
        k: usize,
        icon: Icon,
        hover: String,
        selected: bool,
        select: Selection,
        cmds: &mut Vec<BrowserCmd>,
    ) -> Option<egui::Response> {
        let part = self.part();
        let f = &part.features[k];
        let id = f.id();
        let target = RenameTarget::Feature(id.to_owned());
        tree_icon(ui, icon);
        if self.rename.as_ref() == Some(&target) {
            rename_field(ui, target, cmds);
            return None;
        }
        let sick = part.feature_health.get(id);
        let suppressed = part.is_suppressed(k);
        let rolled = part.is_rolled_back(k);
        let label = match sick {
            Some(_) => format!("! {}", f.name()),
            None => f.name().to_owned(),
        };
        // A condition decides it: `?` before the name.
        let label = if part.suppress_if.contains_key(id) {
            format!("? {label}")
        } else {
            label
        };
        let mut text = RichText::new(label);
        if sick.is_some() {
            text = text.color(WARNING);
        }
        if suppressed || rolled {
            text = text.color(ui.visuals().weak_text_color()).italics();
        }
        if suppressed {
            text = text.strikethrough();
        }
        let mut tip = match sick {
            Some(h) => format!("Warning: {h}"),
            None => hover,
        };
        if suppressed {
            tip.push_str("\nSuppressed (not built)");
        }
        if let Some(c) = part.suppress_if.get(id) {
            let now = match part.check_condition(c) {
                Ok(true) => "true now: suppressed".to_owned(),
                Ok(false) => "false now: built".to_owned(),
                Err(e) => format!("cannot be evaluated: {e}"),
            };
            tip.push_str(&format!("\nSuppress if: {c} ({now})"));
        }
        if rolled {
            tip.push_str("\nBelow the End of Part (rolled back)");
        }
        tip.push_str("\nDrag to reorder · F2 or slow double-click: Rename");
        let r = ui
            .selectable_label(selected, text)
            .interact(egui::Sense::drag())
            .on_hover_text(tip);
        if r.hovered() && !suppressed && !rolled {
            cmds.push(BrowserCmd::Hover(id.to_owned()));
        }
        slow_click(ui, &r, selected, target);
        if r.clicked() {
            cmds.push(BrowserCmd::Select(select));
        }
        if r.double_clicked() && !matches!(f, Feature::Box(_)) && !rolled {
            cmds.push(BrowserCmd::EditFeature(k));
        }
        drop_target(ui, &r, k, cmds);
        Some(r)
    }

    /// The context menu of feature `k` (`used`: the sketches it uses).
    fn feature_menu(
        &self,
        ui: &mut egui::Ui,
        k: usize,
        used: &[usize],
        cmds: &mut Vec<BrowserCmd>,
    ) {
        let part = self.part();
        let f = &part.features[k];
        let rolled = part.is_rolled_back(k);
        if !matches!(f, Feature::Box(_) | Feature::Work(_))
            && ui
                .add_enabled(!rolled, egui::Button::new("Edit Feature"))
                .on_disabled_hover_text("Below the End of Part: move the marker first")
                .clicked()
        {
            cmds.push(BrowserCmd::EditFeature(k));
            ui.close();
        }
        if let Some(&i) = used.first()
            && ui.button("Edit Sketch").clicked()
        {
            cmds.push(BrowserCmd::Edit(i));
            ui.close();
        }
        if let Feature::Work(w) = f
            && matches!(w.def, crate::part::WorkDef::Plane(_))
            && ui.button("New Sketch").clicked()
        {
            cmds.push(BrowserCmd::NewSketchOn(PlaneRef::Work(w.id.clone())));
            ui.close();
        }
        if matches!(f, Feature::Work(_))
            && ui
                .button("Show Inputs")
                .on_hover_text("Select the faces, edges and work features it is built from")
                .clicked()
        {
            cmds.push(BrowserCmd::ShowInputs(k));
            ui.close();
        }
        if let Feature::Work(w) = f
            && matches!(w.def, crate::part::WorkDef::Point(_))
            && !matches!(
                w.def,
                crate::part::WorkDef::Point(crate::part::WorkPointDef::Fixed { .. })
            )
            && ui
                .button("Ground")
                .on_hover_text("Fix the point where it is now (it no longer follows its inputs)")
                .clicked()
        {
            cmds.push(BrowserCmd::GroundPoint(k));
            ui.close();
        }
        if !matches!(f, Feature::Work(_))
            && let Some(a) = self.appearance_submenu(ui)
        {
            cmds.push(BrowserCmd::Look(
                crate::part::appearance::LookTarget::Feature(f.id().to_owned()),
                a,
            ));
            ui.close();
        }
        ui.separator();
        if ui.button("Rename").clicked() {
            cmds.push(BrowserCmd::StartRename(RenameTarget::Feature(
                f.id().to_owned(),
            )));
            ui.close();
        }
        let sup = part.is_suppressed(k);
        if ui
            .button(if sup {
                "Unsuppress Features"
            } else {
                "Suppress Features"
            })
            .on_hover_text(if sup {
                "Build it again (with what it needs and what depends on it)"
            } else {
                "Skip it in the model, with everything that depends on it"
            })
            .clicked()
        {
            cmds.push(BrowserCmd::Suppress(k, !sup));
            ui.close();
        }
        let cond = part.suppress_if.contains_key(f.id());
        if ui
            .button(if cond {
                "Suppress if... (set)"
            } else {
                "Suppress if..."
            })
            .on_hover_text(
                "Suppress it while a true / false equation holds (not Heavy, Width < 100 mm)",
            )
            .clicked()
        {
            cmds.push(BrowserCmd::SuppressIf(k));
            ui.close();
        }
        if ui
            .add_enabled(
                part.eop_index() != k + 1,
                egui::Button::new("Move EOP Here"),
            )
            .on_hover_text("The End of Part right below this feature")
            .clicked()
        {
            cmds.push(BrowserCmd::MoveEop(k + 1));
            ui.close();
        }
        ui.separator();
        if ui.button("Delete").clicked() {
            cmds.push(BrowserCmd::DeleteFeature(k));
            ui.close();
        }
    }

    /// A feature in the browser, with its consumed sketch(es) nested.
    fn feature_node(
        &self,
        ui: &mut egui::Ui,
        k: usize,
        f: &Feature,
        listed: &mut [bool],
        cmds: &mut Vec<BrowserCmd>,
    ) {
        let part = self.part();
        let (icon, hover) = feature_look(f);
        // The box is the body (its size is in the body's properties).
        let (selected, select) = match f {
            Feature::Box(_) => (
                self.selection == Selection::Body || self.selection == Selection::Feature(k),
                Selection::Body,
            ),
            Feature::Pattern(p) => (
                self.selection == Selection::Feature(k)
                    || self
                        .pattern_draft
                        .as_ref()
                        .is_some_and(|d| d.def.features.contains(&p.id)),
                Selection::Feature(k),
            ),
            _ => (
                self.selection == Selection::Feature(k),
                Selection::Feature(k),
            ),
        };
        // Every sketch the feature uses (a Loft's sections and centre line,
        // a Sweep's path): with `sketch()` alone the others fell through to
        // the end of the tree, below the End of Part.
        let ids = f.sketch_ids();
        let mut used: Vec<usize> = (0..part.sketches.len())
            .filter(|&i| ids.contains(&part.sketches[i].id.as_str()))
            .collect();
        // The feature's own sketch first (Edit Sketch in its menu).
        used.sort_by_key(|&i| Some(part.sketches[i].id.as_str()) != f.sketch());
        for &i in &used {
            if part.sketches[i].shared && !listed[i] {
                listed[i] = true;
                self.sketch_row(ui, i, cmds);
            }
            if part.owner_of_sketch(i) == Some(k) {
                listed[i] = true; // nested below, even while collapsed
            }
        }
        let nested: Vec<usize> = used
            .iter()
            .copied()
            .filter(|&i| part.owner_of_sketch(i) == Some(k) || part.sketches[i].shared)
            .collect();
        if nested.is_empty() {
            ui.horizontal(|ui| {
                if let Feature::Work(w) = f {
                    let mut visible = w.visible;
                    if ui
                        .checkbox(&mut visible, "")
                        .on_hover_text("Visibility")
                        .changed()
                    {
                        cmds.push(BrowserCmd::FeatureVisible(k, visible));
                    }
                }
                if let Some(r) = self.feature_label(ui, k, icon, hover, selected, select, cmds) {
                    r.context_menu(|ui| self.feature_menu(ui, k, &used, cmds));
                }
            });
            return;
        }
        // Expanded / collapsed is remembered per feature id.
        egui::collapsing_header::CollapsingState::load_with_default_open(
            ui.ctx(),
            ui.make_persistent_id(("feature", f.id())),
            false,
        )
        .show_header(ui, |ui| {
            if let Some(r) = self.feature_label(ui, k, icon, hover, selected, select, cmds) {
                r.context_menu(|ui| self.feature_menu(ui, k, &used, cmds));
            }
        })
        .body(|ui| {
            for &i in &nested {
                self.sketch_row(ui, i, cmds);
            }
        });
    }

    /// One sketch in the browser: visibility, name, context menu (with
    /// Share / Unshare for sketches used by features).
    fn sketch_row(&self, ui: &mut egui::Ui, i: usize, cmds: &mut Vec<BrowserCmd>) {
        let part = self.part();
        let ps = &part.sketches[i];
        let editing = self.editing == Some(i);
        let consumers = part.consumers(&ps.id).len();
        let rolled = part.sketch_rolled_back(i);
        let target = RenameTarget::Sketch(ps.id.clone());
        ui.horizontal(|ui| {
            let mut visible = ps.visible;
            if ui
                .checkbox(&mut visible, "")
                .on_hover_text("Visibility")
                .changed()
            {
                cmds.push(BrowserCmd::SetVisible(i, visible));
            }
            tree_icon(ui, Icon::Sketch);
            if self.rename.as_ref() == Some(&target) {
                rename_field(ui, target.clone(), cmds);
                return;
            }
            let mut label = ps.name.clone();
            if ps.health.is_some() {
                label = format!("! {label}");
            }
            if ps.shared {
                label.push_str("  (shared)");
            }
            if editing {
                label.push_str("  (editing)");
            }
            let mut text = RichText::new(label);
            if editing {
                text = text.strong().color(self.sketch_color());
            } else if rolled {
                text = text.color(ui.visuals().weak_text_color()).italics();
            } else if ps.health.is_some() {
                text = text.color(WARNING);
            }
            let mut hover = match &ps.health {
                Some(h) => format!("{}\nWarning: {h}", ps.placement.describe()),
                None => ps.placement.describe(),
            };
            if rolled {
                hover.push_str("\nBelow the End of Part (rolled back)");
            }
            let selected = self.selection == Selection::Sketch(i);
            let r = ui.selectable_label(selected, text).on_hover_text(hover);
            slow_click(ui, &r, selected, target.clone());
            if r.clicked() {
                cmds.push(BrowserCmd::Select(Selection::Sketch(i)));
            }
            if r.double_clicked() {
                cmds.push(BrowserCmd::Edit(i));
            }
            r.context_menu(|ui| {
                let mut item = |label: &str, cmd: BrowserCmd| {
                    if ui.button(label).clicked() {
                        cmds.push(cmd);
                        ui.close();
                    }
                };
                if editing {
                    item("Finish 2D Sketch", BrowserCmd::Finish);
                } else {
                    item("Edit Sketch", BrowserCmd::Edit(i));
                }
                if consumers > 0 && !ps.shared {
                    item("Share Sketch", BrowserCmd::Share(i, true));
                }
                if ps.shared && consumers <= 1 {
                    item("Unshare", BrowserCmd::Share(i, false));
                }
                item("Redefine", BrowserCmd::Redefine(i));
                item("Look At", BrowserCmd::LookAt(i));
                item(
                    if ps.visible { "Hide" } else { "Show" },
                    BrowserCmd::SetVisible(i, !ps.visible),
                );
                item("Rename", BrowserCmd::StartRename(target.clone()));
                item("Delete", BrowserCmd::Delete(i));
            });
        });
    }

    /// Properties of the current selection (Inventor-style property panel).
    pub(super) fn properties(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Properties").strong());
        ui.separator();
        if self.is_drawing() {
            self.drawing_properties(ui);
            return;
        }
        if self.is_assembly() {
            self.assembly_properties(ui);
            return;
        }
        if self.sm.draft.is_some() {
            self.sm_panel(ui);
            return;
        }
        if self.sm.flat {
            self.sm_flat_info(ui);
            ui.separator();
        }
        if self.extrude.is_some() {
            self.sync_extrude_direction();
            self.extrude_dialog(ui);
            return;
        }
        if self.work_draft.is_some() {
            self.work_dialog(ui);
            return;
        }
        if self.blend_draft.is_some() {
            self.blend_dialog(ui);
            return;
        }
        if self.shell_draft.is_some() {
            self.shell_dialog(ui);
            return;
        }
        if self.sweep_draft.is_some() {
            self.sweep_dialog(ui);
            return;
        }
        if self.coil_draft.is_some() {
            self.coil_dialog(ui);
            return;
        }
        if self.rib_draft.is_some() {
            self.rib_dialog(ui);
            return;
        }
        if self.loft_draft.is_some() {
            self.loft_dialog(ui);
            return;
        }
        if self.draft_draft.is_some() {
            self.draft_dialog(ui);
            return;
        }
        if self.combine_draft.is_some() {
            self.combine_dialog(ui);
            return;
        }
        if self.part_planned.is_some() {
            self.part_planned_panel(ui);
            return;
        }
        if self.thicken_draft.is_some() {
            self.thicken_panel(ui);
            return;
        }
        if self.delete_draft.is_some() {
            self.delete_face_panel(ui);
            return;
        }
        if self.move_draft.is_some() {
            self.move_bodies_panel(ui);
            return;
        }
        if self.section.as_ref().is_some_and(|s| s.editing) {
            self.section_panel(ui);
            return;
        }
        if self.split_draft.is_some() {
            self.split_dialog(ui);
            return;
        }
        if self.pattern_draft.is_some() {
            self.pattern_feature_dialog(ui);
            return;
        }
        if self.hole_draft.is_some() {
            self.hole_dialog(ui);
            return;
        }
        if self.thread_draft.is_some() {
            self.thread_dialog(ui);
            return;
        }
        if self.editing.is_some() && self.tools.pattern_dialog.is_some() {
            self.pattern_dialog_panel(ui);
            return;
        }
        if self.editing.is_some() && self.tools.has_dialog() {
            self.tool_dialog_panel(ui);
            return;
        }
        egui::ScrollArea::vertical().show(ui, |ui| match self.selection {
            Selection::Feature(k) => self.feature_properties(ui, k),
            Selection::Project => self.project_properties(ui),
            Selection::Body => self.body_properties(ui),
            Selection::Sketch(i) if i < self.part().sketches.len() => self.sketch_properties(ui, i),
            Selection::Sketch(_) => {}
            Selection::Document(id) => {
                if let Some(d) = self.project.document(id) {
                    ui.heading(&d.name);
                    match &d.data {
                        DocumentData::Part(p) => {
                            ui.label(format!("Part · {} sketch(es)", p.sketches.len()))
                        }
                        DocumentData::Assembly(a) => {
                            ui.label(format!("Assembly · {} occurrence(s)", a.occurrences.len()))
                        }
                        DocumentData::Drawing(dr) => {
                            ui.label(format!("Drawing · {} sheet(s)", dr.sheets.len()))
                        }
                    };
                    ui.label(RichText::new("Editing more documents — planned").weak());
                }
            }
        });
    }

    fn project_properties(&mut self, ui: &mut egui::Ui) {
        ui.heading("Project");
        ui.horizontal(|ui| {
            ui.label("Name");
            ui.text_edit_singleline(&mut self.project.name);
        });
        ui.label(format!("Documents: {}", self.project.documents().len()));
        ui.label(format!("Libraries: {}", self.project.libraries().len()));
        ui.label(
            RichText::new(format!("ID {}", self.project.id))
                .small()
                .weak(),
        );
    }

    fn body_properties(&mut self, ui: &mut egui::Ui) {
        let has_box = !self.part().features.is_empty();
        // The part's own name (it said "Box" for any part with a feature).
        let name = self.doc_name(self.part_id);
        ui.heading(if name.is_empty() { "Part" } else { &name });
        if has_box
            && let Some(k) = self
                .part()
                .features
                .iter()
                .position(|f| matches!(f, Feature::Box(_)))
        {
            // The sizes are parameters: expressions, names, the `>` menu.
            let table = self.part().parameter_table();
            let mut fx = Fields {
                table: &table,
                key: format!("feature{k}"),
                commits: Vec::new(),
                bodies: Vec::new(),
            };
            let f = self.part().features[k].clone();
            egui::Grid::new("box_dims").num_columns(2).show(ui, |ui| {
                for (field, label) in [
                    ("length", "Length (X)"),
                    ("width", "Width (Y)"),
                    ("height", "Height (Z)"),
                ] {
                    field_row(ui, &mut fx, label, &f, field);
                }
            });
            let commits = std::mem::take(&mut fx.commits);
            drop(fx);
            for (field, text) in commits {
                self.checkpoint("Edit Feature");
                match self.part_mut().set_feature_field(k, field, &text) {
                    Ok(()) => {
                        if let Err(e) = self.part_mut().regenerate() {
                            self.file_status = Some(Err(e.to_string()));
                        }
                        self.scene_dirty = true;
                    }
                    Err(e) => {
                        self.history.cancel();
                        self.file_status = Some(Err(format!("Edit Feature: {e}")));
                    }
                }
            }
            ui.label(
                RichText::new("Sketches on its faces follow the change.")
                    .small()
                    .weak(),
            );
        }
        ui.separator();
        ui.label(RichText::new("Validation").strong());
        // Validation walks the whole body: once per body, not every frame.
        let key = self.body_key();
        // One row per body: a solid gets the closed-manifold check, a surface
        // body (open on purpose) only its counts.
        type Row = (String, bool, Result<crate::kernel::TopologyStats, String>);
        let cached = ui.ctx().memory(|m| {
            m.data
                .get_temp::<(String, Vec<Row>)>(egui::Id::new("body_validation"))
        });
        let rows = match cached {
            Some((k, r)) if k == key => r,
            _ => {
                let body = &self.part().body;
                let r: Vec<Row> = body
                    .solids()
                    .enumerate()
                    .map(|(i, (id, s))| {
                        let name = s.name.clone().unwrap_or_else(|| format!("Body {}", i + 1));
                        let check = if s.open {
                            body.stats(id).map_err(|e| e.to_string())
                        } else {
                            body.validate_solid(id).map_err(|e| e.to_string())
                        };
                        (name, s.open, check)
                    })
                    .collect();
                ui.ctx().memory_mut(|m| {
                    m.data
                        .insert_temp(egui::Id::new("body_validation"), (key, r.clone()))
                });
                r
            }
        };
        if rows.is_empty() {
            ui.label("No solid");
        }
        let many = rows.len() > 1;
        for (name, open, check) in rows {
            if many {
                ui.label(RichText::new(&name).weak());
            }
            match check {
                Ok(s) if open => {
                    ui.label(format!("V {}   E {}   F {}", s.vertices, s.edges, s.faces));
                    ui.colored_label(
                        Color32::from_rgb(20, 150, 50),
                        "✔ surface body (open, not a solid)",
                    );
                }
                Ok(s) => {
                    ui.label(format!(
                        "V {}   E {}   F {}   shells {}",
                        s.vertices, s.edges, s.faces, s.shells
                    ));
                    ui.label(format!(
                        "Euler–Poincaré: V - E + F = {}, genus {}",
                        s.euler_characteristic(),
                        s.genus().map_or_else(|| "-".to_owned(), |g| g.to_string())
                    ));
                    ui.colored_label(
                        Color32::from_rgb(20, 150, 50),
                        "✔ valid closed manifold solid",
                    );
                }
                Err(e) => {
                    ui.colored_label(Color32::RED, format!("✘ {e}"));
                }
            }
        }
    }

    /// The header of the property panel: icon, title and `?` (F1).
    pub(super) fn panel_header(&mut self, ui: &mut egui::Ui, icon: Icon, title: &str) {
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::hover());
            super::icons::paint(ui.painter(), r, icon, &super::icons::Palette::ENABLED);
            ui.label(RichText::new(title).heading());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button("?")
                    .on_hover_text("Help: the specification of this command (F1)")
                    .clicked()
                {
                    self.open_help();
                }
            });
        });
        ui.separator();
    }

    /// The Extrude / Revolve command's dialog (Inventor's property
    /// panel): profiles (and the axis) are picked in the graphics window,
    /// the options edited here.
    fn extrude_dialog(&mut self, ui: &mut egui::Ui) {
        let Some(d) = self.extrude.as_ref() else {
            return;
        };
        let icon = match d.def {
            super::DraftDef::Extrude(_) => Icon::Extrude,
            super::DraftDef::Revolve(_) => Icon::Revolve,
        };
        let title = d.primitive.map_or(d.def.command(), |k| k.label());
        self.panel_header(ui, icon, title);
        // Breadcrumb (Sketch1 > Extrusion1) and the Advanced Settings menu.
        ui.horizontal(|ui| {
            self.feature_breadcrumb(ui);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.feature_advanced_menu(ui);
            });
        });
        if self.extrude.is_none() {
            return; // the breadcrumb opened the sketch
        }
        self.predict_draft_operation();
        self.extrude_preset_row(ui);
        let Some(d) = self.extrude.as_ref() else {
            return;
        };
        let primitive = d.primitive.is_some();
        let op_before = match &d.def {
            super::DraftDef::Extrude(e) => e.operation,
            super::DraftDef::Revolve(r) => r.operation,
        };
        let curved_to = match &d.def {
            super::DraftDef::Extrude(e) => match &e.extent {
                crate::part::Extent::To(t) => t.is_curved_face(&self.part().body),
                _ => false,
            },
            super::DraftDef::Revolve(_) => false,
        };
        let base = self
            .part()
            .bodies_before(d.edit.unwrap_or(usize::MAX))
            .is_empty();
        let ps = self.part().sketches.get(d.sketch);
        let sketch = ps.map_or_else(String::new, |s| s.name.clone());
        let wax = work_axes(self.part());
        let axis_text = match &d.def {
            super::DraftDef::Revolve(r) if d.axis_set => {
                Some(axis_label(&r.axis, ps.map(|p| &p.sketch)))
            }
            super::DraftDef::Revolve(_) => Some("(none)".to_owned()),
            super::DraftDef::Extrude(_) => None,
        };
        let preview = self.extrude_preview();
        let picking = self
            .target_pick
            .filter(|(o, _)| *o == super::TargetOwner::Draft)
            .map(|(_, s)| s);
        let table = self.part().parameter_table();
        let mut fx = Fields {
            table: &table,
            key: "draft".into(),
            commits: Vec::new(),
            bodies: self.part().bodies_before(usize::MAX),
        };
        let Some(d) = self.extrude.as_mut() else {
            return;
        };
        let mut pick = None;
        egui::ScrollArea::vertical()
            .max_height(ui.available_height() - 90.0)
            .show(ui, |ui| {
                psection(ui, "draft_input", "Input Geometry", |ui| {
                    plabel(ui, "Sketch");
                    ui.label(&sketch);
                    ui.end_row();
                    plabel(ui, "Profiles");
                    if ui
                        .selectable_label(
                            !d.axis_pick,
                            format!("{} selected", d.def.profiles().len()),
                        )
                        .on_hover_text("Click, then pick regions in the graphics window")
                        .clicked()
                    {
                        d.axis_pick = false;
                    }
                    ui.end_row();
                    if let (Some(text), super::DraftDef::Revolve(r)) = (&axis_text, &mut d.def) {
                        plabel(ui, "Axis");
                        let text = if d.axis_pick { "Picking..." } else { text };
                        if ui
                            .selectable_label(d.axis_pick, text)
                            .on_hover_text("Click, then pick a line (centerline) of the sketch")
                            .clicked()
                        {
                            d.axis_pick = true;
                        }
                        ui.end_row();
                        if let Some(axis) =
                            origin_axis_buttons(ui).or_else(|| work_axis_buttons(ui, &wax))
                        {
                            r.axis = axis;
                            d.axis_set = true;
                            d.axis_pick = false;
                        }
                    }
                });
                let ctx = DraftCtx {
                    base,
                    curved_to,
                    primitive,
                };
                match &mut d.def {
                    super::DraftDef::Extrude(e) => {
                        extrude_options(ui, e, picking, &mut pick, &mut fx, ctx);
                    }
                    super::DraftDef::Revolve(r) => {
                        revolve_options(ui, r, &mut fx, picking, &mut pick, ctx);
                    }
                }
                // The user chose an operation: no more prediction.
                let op = match &d.def {
                    super::DraftDef::Extrude(e) => e.operation,
                    super::DraftDef::Revolve(r) => r.operation,
                };
                if op != op_before {
                    d.op_auto = false;
                }
            });
        if let Some(slot) = pick {
            self.target_pick = Some((super::TargetOwner::Draft, slot));
        }
        let commits = std::mem::take(&mut fx.commits);
        drop(fx);
        for (field, text) in commits {
            self.draft_field(field, &text);
        }
        let pending = matches!(&preview, Err(e) if e == super::PREVIEW_PENDING);
        match &preview {
            Err(e) if pending => {
                ui.label(e);
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(50));
            }
            Err(e) => warning_box(ui, e),
            Ok(_) => {}
        }
        // OK while the preview is computed: OK builds the feature itself.
        let ready = preview.is_ok() || pending;
        ui.add_space(4.0);
        // Enter: Continue (Revolve without an axis: pick it) or OK; Ctrl+Enter: OK.
        let key = super::keys::panel_answer(super::keys::take_panel_key(ui), ready);
        if key == Some(super::keys::PanelAnswer::Continue)
            && let Some(d) = self.extrude.as_mut()
            && matches!(d.def, super::DraftDef::Revolve(_))
            && !d.axis_set
        {
            d.axis_pick = true;
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    ready,
                    egui::Button::new("OK").min_size(egui::vec2(70.0, 22.0)),
                )
                .on_hover_text("Create the feature (Enter, Ctrl+Enter)")
                .clicked()
                || key == Some(super::keys::PanelAnswer::Ok)
            {
                self.commit_extrude();
            }
            if ui
                .add(egui::Button::new("Cancel").min_size(egui::vec2(70.0, 22.0)))
                .clicked()
            {
                self.cancel_extrude();
            }
            let editing = self.extrude.as_ref().is_some_and(|d| d.edit.is_some());
            if !editing
                && ui
                    .add_enabled(
                        ready,
                        egui::Button::new("+").min_size(egui::vec2(26.0, 22.0)),
                    )
                    .on_hover_text("Apply (+): create the feature and start the command again")
                    .clicked()
            {
                self.apply_and_continue();
            }
        });
    }

    /// Options of the active sketch tool (and Edit Pattern), shown in its
    /// dialog.
    fn tool_options(&mut self, ui: &mut egui::Ui) {
        if self.tools.tool == super::sketch_tools::Tool::Polygon {
            ui.separator();
            ui.label(RichText::new("Polygon").strong());
            ui.horizontal(|ui| {
                ui.label("Sides");
                ui.add(
                    super::valuefield::ExprValue::new(&mut self.tools.polygon_sides).range(3..=120),
                );
            });
            ui.radio_value(
                &mut self.tools.polygon_inscribed,
                true,
                "Inscribed (vertex on circle)",
            );
            ui.radio_value(
                &mut self.tools.polygon_inscribed,
                false,
                "Circumscribed (sides tangent)",
            );
        }
        if self.tools.tool == super::sketch_tools::Tool::Fillet {
            ui.separator();
            ui.label(RichText::new("2D Fillet").strong());
            ui.horizontal(|ui| {
                ui.label("Radius");
                ui.add(
                    super::valuefield::ExprValue::new(&mut self.tools.fillet_radius)
                        .range(0.001..=1e6)
                        .speed(0.1)
                        .suffix(" mm"),
                );
            });
            ui.label(
                RichText::new("The first fillet gets a dimension; the next ones are Equal to it.")
                    .small()
                    .weak(),
            );
        }
        if self.tools.tool == super::sketch_tools::Tool::Chamfer {
            use super::sketch_tools::ChamferMode;
            ui.separator();
            ui.label(RichText::new("2D Chamfer").strong());
            let t = &mut self.tools;
            ui.radio_value(&mut t.chamfer_mode, ChamferMode::Equal, "Equal distance");
            ui.radio_value(
                &mut t.chamfer_mode,
                ChamferMode::TwoDistances,
                "Two distances",
            );
            ui.radio_value(
                &mut t.chamfer_mode,
                ChamferMode::DistanceAngle,
                "Distance and angle",
            );
            let dist = |ui: &mut egui::Ui, label: &str, v: &mut f64| {
                ui.horizontal(|ui| {
                    ui.label(label);
                    ui.add(
                        super::valuefield::ExprValue::new(v)
                            .range(0.001..=1e6)
                            .speed(0.1)
                            .suffix(" mm"),
                    );
                });
            };
            dist(ui, "Distance 1", &mut t.chamfer_d1);
            match t.chamfer_mode {
                ChamferMode::Equal => {}
                ChamferMode::TwoDistances => dist(ui, "Distance 2", &mut t.chamfer_d2),
                ChamferMode::DistanceAngle => {
                    ui.horizontal(|ui| {
                        ui.label("Angle");
                        ui.add(
                            super::valuefield::ExprValue::new(&mut t.chamfer_angle_deg)
                                .range(0.1..=179.9)
                                .suffix("°"),
                        );
                    });
                }
            }
        }
        if let (Some((pid, base)), Some(si)) = (self.tools.edit_pattern, self.editing) {
            use crate::sketch::PatternKind;
            ui.separator();
            ui.label(RichText::new("Edit Pattern").strong());
            // Geometry: what is patterned (Inventor: the Geometry selector).
            let (source, own) = self.part().sketches[si]
                .sketch
                .pattern(pid)
                .map(|p| {
                    let own: Vec<_> = p.all_entities().collect();
                    (p.source.clone(), own)
                })
                .unwrap_or_default();
            let picked: Vec<_> = self
                .tools
                .selected
                .iter()
                .copied()
                .filter(|e| !own.contains(e) || source.contains(e))
                .collect();
            let mut new_source = None;
            ui.horizontal(|ui| {
                ui.label(format!("Geometry: {} curve(s)", source.len()));
                let add: Vec<_> = picked
                    .iter()
                    .copied()
                    .filter(|e| !source.contains(e))
                    .collect();
                if ui
                    .add_enabled(!add.is_empty(), egui::Button::new("Add Selected"))
                    .on_hover_text("Pattern the selected curves too")
                    .on_disabled_hover_text("Select curves (not copies of this pattern) first")
                    .clicked()
                {
                    new_source = Some(source.iter().copied().chain(add).collect::<Vec<_>>());
                }
                let rest: Vec<_> = source
                    .iter()
                    .copied()
                    .filter(|e| !picked.contains(e))
                    .collect();
                if ui
                    .add_enabled(
                        rest.len() < source.len() && !rest.is_empty(),
                        egui::Button::new("Remove Selected"),
                    )
                    .on_hover_text(
                        "Stop patterning the selected original curves (they stay as geometry)",
                    )
                    .on_disabled_hover_text(
                        "Select curves of the original (at least one must stay)",
                    )
                    .clicked()
                {
                    new_source = Some(rest);
                }
            });
            ui.toggle_value(&mut self.tools.skip_pick, "Skip elements by click")
                .on_hover_text(
                    "SolidWorks' Instances to Skip: while on, a click on a copy suppresses it (click again to restore)",
                );
            if let Some(src) = new_source {
                self.checkpoint("Edit Pattern Geometry");
                match self.part_mut().sketches[si]
                    .sketch
                    .set_pattern_source(pid, &src)
                {
                    Ok(()) => {
                        self.tools.selected.clear();
                        self.solve();
                    }
                    Err(e) => {
                        self.history.cancel();
                        self.file_status = Some(Err(format!("Edit Pattern: {e}")));
                    }
                }
            }
            let t = &mut self.tools;
            match base {
                PatternKind::Rectangular { dir2, .. } => {
                    let dirs = if dir2.is_some() { 2 } else { 1 };
                    for k in 0..dirs {
                        ui.label(format!("Direction {}", k + 1));
                        ui.horizontal(|ui| {
                            ui.label("Count");
                            ui.add(
                                super::valuefield::ExprValue::new(&mut t.pattern_count[k])
                                    .range(1..=500)
                                    .equation(&mut t.pattern_count_eq[k]),
                            );
                            ui.label("Spacing");
                            ui.add(
                                super::valuefield::ExprValue::new(&mut t.pattern_spacing[k])
                                    .range(0.001..=1e6)
                                    .speed(0.1)
                                    .suffix(" mm"),
                            );
                            ui.checkbox(&mut t.pattern_flip[k], "Flip");
                            ui.checkbox(&mut t.pattern_fitted[k], "Fitted");
                        });
                    }
                }
                PatternKind::Path { .. } => {
                    ui.horizontal(|ui| {
                        ui.label("Count");
                        ui.add(super::valuefield::ExprValue::new(&mut t.path_count).range(2..=500));
                        ui.label("Spacing");
                        ui.add(
                            super::valuefield::ExprValue::new(&mut t.path_spacing)
                                .range(0.001..=1e6)
                                .speed(0.1)
                                .suffix(" mm"),
                        );
                    });
                }
                PatternKind::Points { .. } => {
                    ui.label(
                        RichText::new("Pattern at points: move a target point to move its copy")
                            .small()
                            .weak(),
                    );
                }
                PatternKind::Circular { .. } => {
                    ui.horizontal(|ui| {
                        ui.label("Count");
                        ui.add(
                            super::valuefield::ExprValue::new(&mut t.circular_count)
                                .range(2..=500)
                                .equation(&mut t.pattern_count_eq[0]),
                        );
                        ui.label("Angle");
                        ui.add(
                            super::valuefield::ExprValue::new(&mut t.circular_angle_deg)
                                .range(0.1..=360.0)
                                .suffix("°"),
                        );
                        ui.checkbox(&mut t.circular_flip, "Flip");
                        ui.checkbox(&mut t.circular_fitted, "Fitted");
                    });
                }
            }
            ui.label(
                RichText::new("The copies are previewed; suppressed elements stay suppressed.")
                    .small()
                    .weak(),
            );
            ui.horizontal(|ui| {
                if ui.button("OK").on_hover_text("Enter, Ctrl+Enter").clicked()
                    || super::keys::panel_ok_key(ui, true)
                {
                    self.commit_edit_pattern();
                }
                if ui.button("Cancel").clicked() {
                    self.tools.edit_pattern = None;
                }
            });
        }
        if self.tools.tool == super::sketch_tools::Tool::Move {
            ui.separator();
            ui.label(RichText::new("Move").strong());
            ui.checkbox(&mut self.tools.move_copy, "Copy")
                .on_hover_text("Keep the original and move a copy");
        }
        if self.tools.tool == super::sketch_tools::Tool::Scale {
            ui.separator();
            ui.label(RichText::new("Scale").strong());
            ui.horizontal(|ui| {
                ui.label("Scale factor");
                ui.add(
                    super::valuefield::ExprValue::new(&mut self.tools.scale_factor)
                        .range(0.001..=1000.0)
                        .speed(0.01),
                );
            });
        }
        if self.tools.tool == super::sketch_tools::Tool::Offset {
            ui.separator();
            ui.label(RichText::new("Offset").strong());
            ui.checkbox(&mut self.tools.offset_loop, "Loop Select")
                .on_hover_text("Offset the whole chain of connected curves");
            ui.checkbox(&mut self.tools.offset_constrain, "Constrain Offset")
                .on_hover_text("Keep all pieces at one common offset");
        }
    }

    /// The dialog of a sketch command (Inventor: the tool's own dialog):
    /// header, the steps as selectors (Geometry, Base point, ...), options,
    /// Done. The sketch's own properties are hidden meanwhile.
    fn tool_dialog_panel(&mut self, ui: &mut egui::Ui) {
        use super::sketch_tools::Tool;
        let t = self.tools.tool;
        let editing_pattern = self.tools.edit_pattern.is_some() && t == Tool::Select;
        let (icon, title) = if editing_pattern {
            (Icon::PatternRect, "Edit Pattern")
        } else {
            (super::ribbon::tool_icon(t), t.label())
        };
        self.panel_header(ui, icon, title);
        if !editing_pattern {
            // Steps of the transform tools as selectors.
            let steps: &[&str] = match t {
                Tool::Move | Tool::Copy | Tool::Stretch => &["Base point", "Destination"],
                Tool::Rotate => &["Centre", "Reference", "Angle"],
                Tool::Mirror => &["Mirror line"],
                Tool::Scale => &["Base point"],
                _ => &[],
            };
            if !steps.is_empty() {
                let ready = self.tools.transform_ready();
                let n = self.tools.pick_count();
                let count = self.tools.selected.len();
                let mut reselect = false;
                psection(ui, "tooldlg_input", "Input", |ui| {
                    plabel(ui, "Geometry");
                    let text = if count > 0 {
                        format!("{count} curve(s)")
                    } else {
                        "Select".to_owned()
                    };
                    let active = !ready;
                    if ui
                        .selectable_label(active, if active { format!("> {text}") } else { text })
                        .on_hover_text("Click to choose the geometry again (Enter when done)")
                        .clicked()
                    {
                        reselect = true;
                    }
                    ui.end_row();
                    for (k, name) in steps.iter().enumerate() {
                        plabel(ui, name);
                        let active = ready && k == n;
                        let state = if k < n { "Set" } else { "Click in the view" };
                        let _ = ui.selectable_label(
                            active,
                            if active {
                                format!("> {state}")
                            } else {
                                state.to_owned()
                            },
                        );
                        ui.end_row();
                    }
                });
                if reselect {
                    self.tools.reselect_geometry();
                }
                // Precise values (the same fields as next to the cursor).
                let fields = self.hud_fields();
                if !fields.is_empty() {
                    let live = self
                        .last_pick
                        .map(|p| self.hud_live(&p))
                        .unwrap_or([0.0; 2]);
                    let mut apply = false;
                    psection(ui, "tooldlg_values", "Values", |ui| {
                        for (k, name) in fields.iter().enumerate() {
                            plabel(ui, name);
                            let mut v = self.hud.vals[k].unwrap_or(live[k]);
                            let unit = if name.starts_with("Angle") {
                                " °"
                            } else {
                                " mm"
                            };
                            ui.horizontal(|ui| {
                                if ui
                                    .add(super::valuefield::ExprValue::new(&mut v).suffix(unit))
                                    .changed()
                                {
                                    self.hud.vals[k] = Some(v);
                                }
                                if self.hud.vals[k].is_some()
                                    && ui
                                        .small_button("x")
                                        .on_hover_text("Follow the cursor again")
                                        .clicked()
                                {
                                    self.hud.vals[k] = None;
                                }
                            });
                            ui.end_row();
                        }
                    });
                    if ui
                        .button("Apply")
                        .on_hover_text("Place the point with these values (Enter)")
                        .clicked()
                    {
                        apply = true;
                    }
                    if apply && let Some(p) = self.last_pick {
                        let p = self.hud_apply(p);
                        self.hud_click(p);
                    }
                }
            }
        }
        egui::ScrollArea::vertical()
            .max_height(ui.available_height() - 60.0)
            .show(ui, |ui| self.tool_options(ui));
        if !editing_pattern {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui
                    .add(egui::Button::new("Done").min_size(egui::vec2(70.0, 22.0)))
                    .on_hover_text("Finish the command (Esc)")
                    .clicked()
                {
                    self.tools.set_tool(Tool::Select);
                }
            });
        }
    }

    /// The dialog of a sketch pattern command (Inventor's pattern
    /// dialogs): selectors filled by clicks in the graphics window, values,
    /// preview, OK / Cancel.
    fn pattern_dialog_panel(&mut self, ui: &mut egui::Ui) {
        use super::pattern_dialog::{CenterRef, DirRef, Slot};
        use super::sketch_tools::Tool;
        let Some(d) = self.tools.pattern_dialog.clone() else {
            return;
        };
        let icon = match d.tool {
            Tool::PatternRect => Icon::PatternRect,
            Tool::PatternCircular => Icon::PatternCirc,
            Tool::PatternPath => Icon::PatternPath,
            _ => Icon::PatternPoints,
        };
        let title = d.tool.label();
        self.panel_header(ui, icon, title);
        let n_geo = self.tools.selected.len();
        let geometry = n_geo > 0;
        // Preview / error (on a copy of the sketch).
        let check = self.editing.map(|i| {
            let mut tmp = self.part().sketches[i].sketch.clone();
            self.tools.dialog_build(&mut tmp).map(|_| ())
        });
        let mut set_active = None;
        let mut new_dir: Option<(usize, Option<DirRef>)> = None;
        let mut new_center: Option<Option<CenterRef>> = None;
        let mut clear_targets = false;
        // A selector row: the button shows the state and activates it.
        let selector = |ui: &mut egui::Ui, slot: Slot, text: String, active: bool| -> bool {
            plabel(ui, slot.label());
            let r = ui
                .selectable_label(active, if active { format!("> {text}") } else { text })
                .on_hover_text("Click, then pick in the graphics window");
            ui.end_row();
            r.clicked()
        };
        let t = &mut self.tools;
        egui::ScrollArea::vertical()
            .max_height(ui.available_height() - 90.0)
            .show(ui, |ui| {
                psection(ui, "patdlg_input", "Input", |ui| {
                    let g = if geometry {
                        format!("{n_geo} curve(s)")
                    } else {
                        "Select".to_owned()
                    };
                    if selector(ui, Slot::Geometry, g, d.active == Slot::Geometry) {
                        set_active = Some(Slot::Geometry);
                    }
                    let dir_text = |r: Option<DirRef>| match r {
                        None => "Select".to_owned(),
                        Some(DirRef::Line(_)) => "Line".to_owned(),
                        Some(DirRef::AxisX) => "Sketch X axis".to_owned(),
                        Some(DirRef::AxisY) => "Sketch Y axis".to_owned(),
                    };
                    match d.tool {
                        Tool::PatternRect => {
                            for (k, slot) in [Slot::Dir1, Slot::Dir2].into_iter().enumerate() {
                                if selector(ui, slot, dir_text(d.dir[k]), d.active == slot) {
                                    set_active = Some(slot);
                                }
                                plabel(ui, "");
                                ui.horizontal(|ui| {
                                    if ui.small_button("X").on_hover_text("Sketch X axis").clicked() {
                                        new_dir = Some((k, Some(DirRef::AxisX)));
                                    }
                                    if ui.small_button("Y").on_hover_text("Sketch Y axis").clicked() {
                                        new_dir = Some((k, Some(DirRef::AxisY)));
                                    }
                                    if k == 1
                                        && d.dir[1].is_some()
                                        && ui.small_button("x").on_hover_text("No direction 2").clicked()
                                    {
                                        new_dir = Some((1, None));
                                    }
                                    ui.checkbox(&mut t.pattern_flip[k], "Flip");
                                });
                                ui.end_row();
                                if k == 0 || d.dir[1].is_some() {
                                    plabel(ui, "Count");
                                    ui.add(
                                        super::valuefield::ExprValue::new(&mut t.pattern_count[k])
                                            .range(1..=500)
                                            .equation(&mut t.pattern_count_eq[k]),
                                    );
                                    ui.end_row();
                                    plabel(ui, if t.pattern_fitted[k] { "Length" } else { "Spacing" });
                                    ui.add(
                                        super::valuefield::ExprValue::new(&mut t.pattern_spacing[k])
                                            .range(0.001..=1e6)
                                            .speed(0.1)
                                            .suffix(" mm")
                                            .equation(&mut t.pattern_spacing_eq[k]),
                                    );
                                    ui.end_row();
                                    // Its own row: beside the field it was cut off.
                                    plabel(ui, "");
                                    ui.checkbox(&mut t.pattern_fitted[k], "Fitted")
                                        .on_hover_text("The value is the overall length");
                                    ui.end_row();
                                }
                            }
                        }
                        Tool::PatternCircular => {
                            let ct = match d.center {
                                None => "Select".to_owned(),
                                Some(CenterRef::Point(_)) => "Point".to_owned(),
                                Some(CenterRef::Radius) => "By radius".to_owned(),
                            };
                            if selector(ui, Slot::Center, ct, d.active == Slot::Center) {
                                set_active = Some(Slot::Center);
                            }
                            plabel(ui, "");
                            ui.horizontal(|ui| {
                                if ui
                                    .selectable_label(d.center == Some(CenterRef::Radius), "By radius")
                                    .on_hover_text("A new centre at this radius and angle from the geometry (SolidWorks)")
                                    .clicked()
                                {
                                    new_center = Some(Some(CenterRef::Radius));
                                }
                            });
                            ui.end_row();
                            if d.center == Some(CenterRef::Radius) {
                                plabel(ui, "Radius");
                                ui.add(
                                    super::valuefield::ExprValue::new(&mut t.circular_radius)
                                        .range(0.01..=1e6)
                                        .speed(0.1)
                                        .suffix(" mm"),
                                );
                                ui.end_row();
                                plabel(ui, "at angle");
                                ui.add(super::valuefield::ExprValue::new(&mut t.circular_ref_deg).suffix("°"));
                                ui.end_row();
                            }
                            plabel(ui, "Count");
                            ui.add(
                                super::valuefield::ExprValue::new(&mut t.circular_count)
                                    .range(2..=500)
                                    .equation(&mut t.pattern_count_eq[0]),
                            );
                            ui.end_row();
                            plabel(ui, "Angle");
                            ui.add(
                                super::valuefield::ExprValue::new(&mut t.circular_angle_deg)
                                    .range(0.1..=360.0)
                                    .suffix("°")
                                    .equation(&mut t.circular_angle_eq),
                            );
                            ui.end_row();
                            plabel(ui, "");
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut t.circular_flip, "Flip");
                                ui.checkbox(&mut t.circular_fitted, "Fitted");
                            });
                            ui.end_row();
                        }
                        Tool::PatternPath => {
                            let pt = if d.path.is_some() { "Curve" } else { "Select" };
                            if selector(ui, Slot::Path, pt.to_owned(), d.active == Slot::Path) {
                                set_active = Some(Slot::Path);
                            }
                            plabel(ui, "Count");
                            ui.add(super::valuefield::ExprValue::new(&mut t.path_count).range(2..=500));
                            ui.end_row();
                            plabel(ui, "Spacing");
                            ui.add(
                                super::valuefield::ExprValue::new(&mut t.path_spacing)
                                    .range(0.001..=1e6)
                                    .speed(0.1)
                                    .suffix(" mm"),
                            );
                            ui.end_row();
                        }
                        _ => {
                            let bt = if d.base.is_some() { "Point" } else { "Select" };
                            if selector(ui, Slot::Base, bt.to_owned(), d.active == Slot::Base) {
                                set_active = Some(Slot::Base);
                            }
                            let tt = format!("{} point(s)", d.targets.len());
                            if selector(ui, Slot::Targets, tt, d.active == Slot::Targets) {
                                set_active = Some(Slot::Targets);
                            }
                            if !d.targets.is_empty() {
                                plabel(ui, "");
                                if ui.small_button("Clear targets").clicked() {
                                    clear_targets = true;
                                }
                                ui.end_row();
                            }
                        }
                    }
                });
            });
        if let Some(dd) = self.tools.pattern_dialog.as_mut() {
            if let Some(s) = set_active {
                dd.active = s;
            }
            if let Some((k, r)) = new_dir {
                dd.dir[k] = r;
                if k == 1 && r.is_some() && self.tools.pattern_count[1] < 2 {
                    self.tools.pattern_count[1] = 2;
                }
                if k == 0 && r.is_some() && dd.active == Slot::Dir1 {
                    dd.active = Slot::Dir2;
                }
            }
            if let Some(c) = new_center {
                dd.center = c;
            }
            if clear_targets {
                dd.targets.clear();
            }
        }
        if let Some(Err(e)) = &check
            && d.ready(geometry)
        {
            warning_box(ui, &e.to_string());
        }
        let ready = matches!(check, Some(Ok(())));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    ready,
                    egui::Button::new("OK").min_size(egui::vec2(70.0, 22.0)),
                )
                .on_hover_text("Enter, Ctrl+Enter")
                .clicked()
                || super::keys::panel_ok_key(ui, ready)
            {
                self.tool_op(|t, ps| t.dialog_ok(ps));
            }
            if ui
                .add(egui::Button::new("Cancel").min_size(egui::vec2(70.0, 22.0)))
                .clicked()
            {
                self.tools.set_tool(super::sketch_tools::Tool::Select);
            }
        });
    }

    /// Properties of a feature selected in the browser (edit in place, as
    /// Inventor's Edit Feature).
    fn feature_properties(&mut self, ui: &mut egui::Ui, k: usize) {
        let Some(f) = self.part().features.get(k).cloned() else {
            return;
        };
        if let Feature::Work(w) = &f {
            self.work_properties(ui, k, w.clone());
            return;
        }
        if let Feature::Blend(b) = &f {
            self.blend_properties(ui, k, b.clone());
            return;
        }
        if let Feature::Shell(s) = &f {
            self.shell_properties(ui, k, s.clone());
            return;
        }
        if let Feature::Sweep(s) = &f {
            self.sweep_properties(ui, k, s.clone());
            return;
        }
        if let Feature::Coil(c) = &f {
            self.coil_properties(ui, k, c.clone());
            return;
        }
        if let Feature::Rib(r) = &f {
            self.rib_properties(ui, k, r.clone());
            return;
        }
        if let Feature::Loft(l) = &f {
            self.loft_properties(ui, k, l.clone());
            return;
        }
        if let Feature::Draft(d) = &f {
            self.draft_properties(ui, k, d.clone());
            return;
        }
        if matches!(f, Feature::Combine(_) | Feature::Split(_)) {
            self.body_cmd_properties(ui, k);
            return;
        }
        if let Feature::Pattern(p) = &f {
            self.pattern_properties(ui, k, p.clone());
            return;
        }
        if let Feature::Hole(h) = &f {
            self.hole_properties(ui, k, h.clone());
            return;
        }
        if let Feature::Thread(t) = &f {
            self.thread_properties(ui, k, t.clone());
            return;
        }
        if matches!(
            f,
            Feature::SheetFlange(_) | Feature::SheetOp(_) | Feature::SheetConvert(_)
        ) {
            self.sm_feature_properties(ui, k);
            return;
        }
        if matches!(f, Feature::BodyOp(_)) {
            self.thicken_properties(ui, k);
            return;
        }
        let Some(sketch_id) = f.sketch().map(str::to_owned) else {
            return;
        };
        let icon = match &f {
            Feature::Revolve(_) => Icon::Revolve,
            _ => Icon::Extrude,
        };
        self.panel_header(ui, icon, f.name());
        let ps = self
            .part()
            .sketches
            .iter()
            .find(|s| s.id == sketch_id)
            .cloned();
        let sketch = ps
            .as_ref()
            .map_or_else(|| format!("{sketch_id} (missing)"), |s| s.name.clone());
        if let Some(h) = self.part().feature_health.get(f.id()) {
            warning_box(ui, h);
        }
        let mut axis_change = None;
        let wax = work_axes(self.part());
        psection(ui, &format!("feature_input_{k}"), "Input Geometry", |ui| {
            plabel(ui, "Sketch");
            ui.label(&sketch);
            ui.end_row();
            match &f {
                Feature::Extrude(e) => {
                    plabel(ui, "Profiles");
                    ui.label(format!("{}", e.profiles.len()));
                    ui.end_row();
                }
                Feature::Revolve(r) => {
                    plabel(ui, "Profiles");
                    ui.label(format!("{}", r.profiles.len()));
                    ui.end_row();
                    plabel(ui, "Axis");
                    ui.label(axis_label(&r.axis, ps.as_ref().map(|p| &p.sketch)));
                    ui.end_row();
                    axis_change = origin_axis_buttons(ui).or_else(|| work_axis_buttons(ui, &wax));
                }
                Feature::Box(_)
                | Feature::Work(_)
                | Feature::Blend(_)
                | Feature::Shell(_)
                | Feature::Draft(_)
                | Feature::Pattern(_)
                | Feature::Hole(_)
                | Feature::Thread(_)
                | Feature::Sweep(_)
                | Feature::Loft(_)
                | Feature::Coil(_)
                | Feature::Rib(_)
                | Feature::SheetFace(_)
                | Feature::SheetFlange(_)
                | Feature::SheetConvert(_)
                | Feature::SheetOp(_)
                | Feature::BodyOp(_)
                | Feature::Combine(_)
                | Feature::Split(_) => {}
            }
        });
        let table = self.part().parameter_table();
        let mut fx = Fields {
            table: &table,
            key: format!("feature{k}"),
            commits: Vec::new(),
            bodies: self.part().bodies_before(k),
        };
        match f {
            Feature::Extrude(e) => {
                let picking = self
                    .target_pick
                    .filter(|(o, _)| *o == super::TargetOwner::Feature(k))
                    .map(|(_, s)| s);
                let mut def = e.clone();
                let mut pick = None;
                let ctx = DraftCtx {
                    base: fx.bodies.is_empty(),
                    curved_to: matches!(&e.extent, crate::part::Extent::To(t) if t.is_curved_face(&self.part().body)),
                    primitive: false,
                };
                if extrude_options(ui, &mut def, picking, &mut pick, &mut fx, ctx) {
                    self.edit_extrude(k, def);
                }
                if let Some(slot) = pick {
                    self.target_pick = Some((super::TargetOwner::Feature(k), slot));
                }
            }
            Feature::Revolve(r) => {
                let mut def = r.clone();
                let mut changed = false;
                if let Some(axis) = axis_change {
                    def.axis = axis;
                    changed = true;
                }
                let picking = self
                    .target_pick
                    .filter(|(o, _)| *o == super::TargetOwner::Feature(k))
                    .map(|(_, s)| s);
                let mut pick = None;
                let ctx = DraftCtx {
                    base: fx.bodies.is_empty(),
                    curved_to: false,
                    primitive: false,
                };
                changed |= revolve_options(ui, &mut def, &mut fx, picking, &mut pick, ctx);
                if changed {
                    self.edit_feature(k, Feature::Revolve(def));
                }
                if let Some(slot) = pick {
                    self.target_pick = Some((super::TargetOwner::Feature(k), slot));
                }
            }
            Feature::SheetFlange(ref fl) => {
                let extents = fl.extents;
                let centered = fl.centered;
                psection(ui, &format!("flange_{k}"), "Flange", |ui| {
                    field_row(ui, &mut fx, "Height", &f, "height");
                    ui.end_row();
                    field_row(ui, &mut fx, "Angle", &f, "angle");
                    ui.end_row();
                    use crate::part::sheet_metal::FlangeWidth;
                    match extents {
                        FlangeWidth::Edge => {}
                        FlangeWidth::Width => {
                            field_row(ui, &mut fx, "Width", &f, "width");
                            ui.end_row();
                            if !centered {
                                field_row(ui, &mut fx, "Offset", &f, "offset1");
                                ui.end_row();
                            }
                        }
                        FlangeWidth::Offset => {
                            field_row(ui, &mut fx, "Offset 1", &f, "offset1");
                            ui.end_row();
                            field_row(ui, &mut fx, "Offset 2", &f, "offset2");
                            ui.end_row();
                        }
                    }
                });
            }
            Feature::BodyOp(_) => {}
            Feature::SheetOp(ref o) => {
                let fields = o.fields();
                let title = o.describe();
                psection(ui, &format!("sheetop_{k}"), "Sheet Metal", |ui| {
                    for (field, _) in fields {
                        let label = match field {
                            "length" => "Length",
                            "gap" => "Gap",
                            "angle" => "Angle",
                            "width" => "Width",
                            other => other,
                        };
                        field_row(ui, &mut fx, label, &f, field);
                        ui.end_row();
                    }
                });
                ui.weak(title);
            }
            Feature::SheetConvert(ref c) => {
                ui.label(format!(
                    "Fixed face {}, {} bend(s); thickness from Sheet Metal Defaults (a shelled body keeps its walls)",
                    c.face.face,
                    c.bends.len()
                ));
            }
            Feature::SheetFace(_) => {
                let t = crate::part::sheet_metal::rule_of(self.part()).thickness;
                ui.label(format!("Thickness {t:.3} mm (Sheet Metal Defaults)"));
            }
            Feature::Box(_)
            | Feature::Work(_)
            | Feature::Blend(_)
            | Feature::Shell(_)
            | Feature::Draft(_)
            | Feature::Pattern(_)
            | Feature::Hole(_)
            | Feature::Thread(_)
            | Feature::Sweep(_)
            | Feature::Loft(_)
            | Feature::Coil(_)
            | Feature::Rib(_)
            | Feature::Combine(_)
            | Feature::Split(_) => {}
        }
        let commits = std::mem::take(&mut fx.commits);
        drop(fx);
        for (field, text) in commits {
            self.checkpoint("Edit Feature");
            match self.part_mut().set_feature_field(k, field, &text) {
                Ok(()) => {
                    if let Err(e) = self.part_mut().regenerate() {
                        self.file_status = Some(Err(e.to_string()));
                    }
                    self.scene_dirty = true;
                }
                Err(e) => {
                    self.history.cancel();
                    self.file_status = Some(Err(format!("Edit Feature: {e}")));
                }
            }
        }
        ui.add_space(4.0);
        if ui.button("Delete").clicked() {
            self.delete_feature(k);
        }
    }

    /// The Fillet / Chamfer dialog: edges, size (method for chamfers),
    /// OK / Apply / Cancel.
    fn blend_dialog(&mut self, ui: &mut egui::Ui) {
        use crate::part::{BlendKind, ChamferMethod};
        let Some(mut d) = self.blend_draft.clone() else {
            return;
        };
        let (icon, cmd) = match d.def.kind {
            BlendKind::Fillet { .. } => (Icon::FilletEdge, "Fillet"),
            BlendKind::Chamfer { .. } => (Icon::ChamferEdge, "Chamfer"),
        };
        self.panel_header(ui, icon, cmd);
        self.preset_row(ui, cmd, &mut d.def);
        use super::blend::EdgeMode;
        use crate::part::select::SolidEdges;
        use crate::part::{FilletSet, FilletType};
        let is_fillet = matches!(d.def.kind, BlendKind::Fillet { .. });
        let mut solid_pick = None;
        psection(ui, "blend_edges", "Edges", |ui| {
            if is_fillet {
                plabel(ui, "Type");
                ui.vertical(|ui| {
                    for (t, label, tip) in [
                        (FilletType::Constant, "Constant", "One radius per edge set"),
                        (FilletType::Variable, "Variable", "The radius changes along each straight edge"),
                        (FilletType::Chord, "Chord", "Constant blend width across the edge instead of a radius (Creo: Chordal)"),
                        (FilletType::FullRound, "Full Round", "Replace a face between two parallel side faces by a round"),
                    ] {
                        if ui.radio(d.def.fillet_type == t, label).on_hover_text(tip).clicked()
                            && d.def.fillet_type != t
                        {
                            d.def.fillet_type = t;
                            d.set = 0;
                        }
                    }
                });
                ui.end_row();
            }
            let ft = d.def.fillet_type;
            if ft != FilletType::FullRound {
                plabel(ui, "Select");
                egui::ComboBox::from_id_salt("blend_mode")
                    .selected_text(match d.mode {
                        EdgeMode::Edge => "Edge",
                        EdgeMode::Loop => "Loop",
                        EdgeMode::Feature => "Feature",
                        EdgeMode::Solid => "Solid",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut d.mode, EdgeMode::Edge, "Edge")
                            .on_hover_text("Click edges one by one");
                        ui.selectable_value(&mut d.mode, EdgeMode::Loop, "Loop")
                            .on_hover_text("Click a face: its loop nearest the click");
                        ui.selectable_value(&mut d.mode, EdgeMode::Feature, "Feature")
                            .on_hover_text(
                                "Click a face: the edges where its feature meets the others",
                            );
                        ui.selectable_value(&mut d.mode, EdgeMode::Solid, "Solid")
                            .on_hover_text(
                                "All edges, all fillets (concave) or all rounds (convex)",
                            );
                    });
                ui.end_row();
                if d.mode == EdgeMode::Solid {
                    plabel(ui, "");
                    ui.vertical(|ui| {
                        if ui.small_button("All Edges").clicked() {
                            solid_pick = Some(SolidEdges::All);
                        }
                        if ui
                            .small_button("All Fillets")
                            .on_hover_text("Concave edges")
                            .clicked()
                        {
                            solid_pick = Some(SolidEdges::Fillets);
                        }
                        if ui
                            .small_button("All Rounds")
                            .on_hover_text("Convex edges")
                            .clicked()
                        {
                            solid_pick = Some(SolidEdges::Rounds);
                        }
                    });
                    ui.end_row();
                }
            }
            match ft {
                FilletType::FullRound => {
                    plabel(ui, "Middle face");
                    match &d.def.full_round {
                        Some(n) => {
                            ui.label(n.clone());
                        }
                        None => {
                            ui.label("click a face");
                        }
                    }
                    ui.end_row();
                }
                FilletType::Variable => {
                    plabel(ui, "Edges");
                    ui.horizontal(|ui| {
                        ui.label(format!("{} selected", d.def.variable.len()));
                        if !d.def.variable.is_empty() && ui.small_button("Clear").clicked() {
                            d.def.variable.clear();
                        }
                    });
                    ui.end_row();
                    plabel(ui, "Transition");
                    ui.checkbox(&mut d.def.smooth, "Smooth radius")
                        .on_hover_text(
                            "Gradual (tangent) change between the points; off: straight",
                        );
                    ui.end_row();
                }
                _ => {
                    let n = 1 + d.def.sets.len();
                    let mut remove = None;
                    for k in 0..n {
                        let count = if k == 0 {
                            d.def.edges.len()
                        } else {
                            d.def.sets[k - 1].edges.len()
                        };
                        plabel(ui, if k == 0 { "Edges" } else { "" });
                        ui.horizontal(|ui| {
                            if n > 1 {
                                ui.radio_value(&mut d.set, k, format!("Set {}", k + 1))
                                    .on_hover_text("Clicks go to this set");
                            }
                            ui.label(format!("{count} selected"));
                            if count > 0 && ui.small_button("Clear").clicked() {
                                if k == 0 {
                                    d.def.edges.clear();
                                } else {
                                    d.def.sets[k - 1].edges.clear();
                                }
                            }
                            if k > 0
                                && ui
                                    .small_button("x")
                                    .on_hover_text("Remove the set")
                                    .clicked()
                            {
                                remove = Some(k);
                            }
                        });
                        ui.end_row();
                    }
                    if let Some(k) = remove {
                        d.def.sets.remove(k - 1);
                        d.set = 0;
                    }
                    if is_fillet && ft == FilletType::Constant {
                        let last_empty = d
                            .def
                            .sets
                            .last()
                            .map_or(d.def.edges.is_empty(), |s| s.edges.is_empty());
                        plabel(ui, "");
                        if ui
                            .add_enabled(
                                !last_empty && d.def.sets.len() < 8,
                                egui::Button::new("Add set"),
                            )
                            .on_hover_text("A further edge set with its own radius")
                            .clicked()
                        {
                            let r = match d.def.kind {
                                BlendKind::Fillet { radius } => radius,
                                _ => 1.0,
                            };
                            d.def.sets.push(FilletSet {
                                edges: Vec::new(),
                                radius: r,
                            });
                            d.set = d.def.sets.len();
                        }
                        ui.end_row();
                    }
                }
            }
            if d.mode == EdgeMode::Edge && ft != FilletType::FullRound && ft != FilletType::Variable
            {
                plabel(ui, "Edge chain");
                ui.checkbox(&mut d.chain, "Automatic")
                    .on_hover_text("A click takes all tangentially connected edges");
                ui.end_row();
            }
        });
        // Variable: start/end radius and points per edge.
        if d.def.fillet_type == FilletType::Variable && !d.def.variable.is_empty() {
            psection(ui, "blend_variable", "Radius along the edges", |ui| {
                for (k, v) in d.def.variable.iter_mut().enumerate() {
                    plabel(ui, &format!("Edge {}", k + 1));
                    ui.label("");
                    ui.end_row();
                    let n = v.points.len();
                    let mut remove = None;
                    for (j, p) in v.points.iter_mut().enumerate() {
                        let label = match j {
                            0 => "Start".to_owned(),
                            j if j + 1 == n => "End".to_owned(),
                            j => format!("Point {j}"),
                        };
                        plabel(ui, &label);
                        ui.horizontal(|ui| {
                            if j != 0 && j + 1 != n {
                                ui.add(
                                    super::valuefield::ExprValue::new(&mut p[0])
                                        .range(0.001..=0.999)
                                        .speed(0.01),
                                )
                                .on_hover_text("Position along the edge, 0 (start) to 1 (end)");
                            }
                            ui.add(
                                super::valuefield::ExprValue::new(&mut p[1])
                                    .range(0.0..=1e5)
                                    .speed(0.1)
                                    .suffix(" mm"),
                            );
                            if j != 0 && j + 1 != n && ui.small_button("x").clicked() {
                                remove = Some(j);
                            }
                        });
                        ui.end_row();
                    }
                    if let Some(j) = remove {
                        v.points.remove(j);
                    }
                    plabel(ui, "");
                    if ui
                        .small_button("Add point")
                        .on_hover_text("A point halfway along the widest gap")
                        .clicked()
                    {
                        // Into the widest gap, at its middle and mean radius.
                        if let Some(g) = (0..v.points.len() - 1).max_by(|&a, &b| {
                            (v.points[a + 1][0] - v.points[a][0])
                                .total_cmp(&(v.points[b + 1][0] - v.points[b][0]))
                        }) {
                            let (a, b) = (v.points[g], v.points[g + 1]);
                            v.points
                                .insert(g + 1, [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]);
                        }
                    }
                    ui.end_row();
                    v.points.sort_by(|a, b| a[0].total_cmp(&b[0]));
                }
            });
        }
        // Values go through expression fields (parameters, `>` menu); the
        // equations stay with the feature (like Extrude's).
        let table = self.part().parameter_table();
        let mut fx = Fields {
            table: &table,
            key: "blend_draft".into(),
            commits: Vec::new(),
            bodies: Vec::new(),
        };
        let snapshot = crate::part::Feature::Blend(d.def.clone());
        // Partial chamfer: the one straight edge's start and length.
        let single_edge: Option<([f64; 3], f64)> = match (&d.def.kind, &d.def.edges[..]) {
            (BlendKind::Chamfer { .. }, [r]) => {
                let body = &self.part().body;
                self.edges_of_ref(r).first().and_then(|&e| {
                    let x = body.edge(e)?;
                    let a = body.vertex(x.start)?.point;
                    let b = body.vertex(x.end)?.point;
                    crate::part::select::edge_is_straight(body, e)
                        .then(|| ([a.x, a.y, a.z], (b - a).norm()))
                })
            }
            _ => None,
        };
        let mut partial_toggle: Option<bool> = None;
        let partial_on = d.def.partial.is_some();
        let cone = &mut d.def.cone_corners;
        psection(ui, "blend_size", "Size", |ui| {
            if let BlendKind::Chamfer { method, flip, .. } = &mut d.def.kind {
                plabel(ui, "Method");
                ui.vertical(|ui| {
                    ui.radio_value(method, ChamferMethod::Distance, "Distance");
                    ui.radio_value(method, ChamferMethod::TwoDistances, "Two Distances");
                    ui.radio_value(method, ChamferMethod::DistanceAngle, "Distance and Angle");
                });
                ui.end_row();
                if *method != ChamferMethod::Distance {
                    plabel(ui, "Flip");
                    ui.checkbox(flip, "")
                        .on_hover_text("Swap the faces the values apply to");
                    ui.end_row();
                }
                plabel(ui, "Partial");
                let mut on = partial_on;
                if ui
                    .add_enabled(
                        single_edge.is_some() || on,
                        egui::Checkbox::new(&mut on, ""),
                    )
                    .on_hover_text("Chamfer only part of the edge (one straight edge)")
                    .on_disabled_hover_text("Select one straight edge")
                    .changed()
                {
                    partial_toggle = Some(on);
                }
                ui.end_row();
            }
            let chord = matches!(snapshot, crate::part::Feature::Blend(ref b) if b.fillet_type == crate::part::FilletType::Chord);
            for field in crate::part::params::feature_field_names(&snapshot) {
                let set_label;
                let label = match field {
                    "radius" if chord => "Chord",
                    "radius" if !matches!(snapshot, crate::part::Feature::Blend(ref b) if b.sets.is_empty()) => {
                        "Radius 1"
                    }
                    "radius" => "Radius",
                    f if f.starts_with("radius") => {
                        set_label = format!("Radius {}", &f[6..]);
                        set_label.as_str()
                    }
                    "distance" => "Distance",
                    "distance2" => "Distance 2",
                    "angle" => "Angle",
                    "to_start" => "To Start",
                    "partial_length" => "Chamfer length",
                    other => other,
                };
                field_row(ui, &mut fx, label, &snapshot, field);
            }
            if let (crate::part::Feature::Blend(b), Some((_, len))) = (&snapshot, single_edge)
                && let Some(p) = &b.partial
            {
                plabel(ui, "To End");
                let rest = len - p.to_start - p.length;
                ui.label(format!("{rest:.3} mm (of {len:.3})"))
                    .on_hover_text(
                        "Driven: the edge's length less To Start and the chamfer length",
                    );
                ui.end_row();
            }
            if matches!(snapshot, crate::part::Feature::Blend(ref b) if matches!(b.kind, BlendKind::Chamfer { .. }))
            {
                plabel(ui, "Corners");
                ui.checkbox(cone, "Cone at small rounds")
                    .on_hover_text("Where the chamfer is larger than a round of the loop: follow the round as a cone up to its tip. Off: the flat bevels run on through the corner and cut the round.");
                ui.end_row();
            }
        });
        let commits = std::mem::take(&mut fx.commits);
        drop(fx);
        match (partial_toggle, single_edge) {
            (Some(true), Some((start, len))) => {
                d.def.partial = Some(crate::part::PartialChamfer {
                    start,
                    to_start: 0.0,
                    length: len / 2.0,
                });
            }
            (Some(false), _) => d.def.partial = None,
            _ => {}
        }
        for (field, text) in commits {
            let mut f = crate::part::Feature::Blend(d.def.clone());
            match self.part().apply_draft_field(&mut f, field, &text) {
                Ok(()) => {
                    if let crate::part::Feature::Blend(b) = f {
                        d.def = b;
                    }
                    self.file_status = None;
                }
                Err(e) => {
                    self.file_status = Some(Err(format!("{}: {e}", d.def.command())));
                }
            }
        }
        self.blend_draft = Some(d.clone());
        if let Some(which) = solid_pick {
            self.blend_solid_pick(which);
            if let Some(nd) = &self.blend_draft {
                d = nd.clone();
            }
        }
        let preview = self.blend_preview();
        match &preview {
            Err(e) if e == super::PREVIEW_PENDING => {
                ui.label(e);
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(50));
            }
            Err(e) if d.def.has_input() => warning_box(ui, e),
            _ => {}
        }
        ui.horizontal(|ui| {
            let ok = preview.is_ok() || matches!(&preview, Err(e) if e == super::PREVIEW_PENDING);
            if ui
                .add_enabled(ok, egui::Button::new("OK").min_size(egui::vec2(60.0, 22.0)))
                .on_hover_text("Enter, Ctrl+Enter")
                .clicked()
                || super::keys::panel_ok_key(ui, ok)
            {
                self.commit_blend(false);
            }
            if ui
                .add_enabled(
                    ok,
                    egui::Button::new("Apply").min_size(egui::vec2(60.0, 22.0)),
                )
                .on_hover_text("Create it and go on with new edges")
                .clicked()
            {
                self.commit_blend(true);
            }
            if ui
                .add(egui::Button::new("Cancel").min_size(egui::vec2(60.0, 22.0)))
                .clicked()
            {
                self.blend_draft = None;
                self.end_feature_edit();
            }
        });
    }

    /// The Shell command's panel (`docs/spec/2.6-shell.md`): breadcrumb,
    /// shell type, direction, thickness, faces to remove (selector,
    /// `gui/command_ui.rs`), Advanced Properties; preview, OK / Apply /
    /// Cancel.
    fn shell_dialog(&mut self, ui: &mut egui::Ui) {
        use super::command_ui::{name_crumb, selector_rows};
        use crate::part::{ShellCorners, ShellDirection};
        let Some(mut d) = self.shell_draft.clone() else {
            return;
        };
        self.panel_header(ui, Icon::Shell, "Shell");
        let shown = match d.edit {
            Some(k) => self
                .part()
                .features
                .get(k)
                .map_or("Shell".to_owned(), |f| f.name().to_owned()),
            None => self.part().next_feature_id("Shell"),
        };
        name_crumb(ui, &mut d.crumb, &shown);
        self.preset_row(ui, "Shell", &mut d.def);
        let bodies = self
            .part()
            .bodies_before(d.edit.unwrap_or(self.part().insert_index()));
        psection(ui, "shell_type", "Shell Type", |ui| {
            plabel(ui, "Type");
            ui.horizontal(|ui| {
                for (c, tip) in [
                    (ShellCorners::Sharp, "The offset corners stay sharp"),
                    (
                        ShellCorners::Rounded,
                        "The corners the offset makes are rounded by the offset distance (outer convex corners when offset out, inner concave ones when offset in)",
                    ),
                ] {
                    if ui
                        .selectable_label(d.def.corners == c, c.label())
                        .on_hover_text(tip)
                        .clicked()
                    {
                        d.def.corners = c;
                    }
                }
            });
            ui.end_row();
        });
        let table = self.part().parameter_table();
        let mut fx = Fields {
            table: &table,
            key: "shell_draft".into(),
            commits: Vec::new(),
            bodies: Vec::new(),
        };
        let snapshot = Feature::Shell(d.def.clone());
        psection(ui, "shell_behavior", "Behavior", |ui| {
            plabel(ui, "Direction");
            ui.vertical(|ui| {
                for (dir, tip) in [
                    (
                        ShellDirection::Inside,
                        "The walls grow into the material; the outside stays",
                    ),
                    (
                        ShellDirection::Outside,
                        "The walls grow out; the faces become the inner walls",
                    ),
                    (
                        ShellDirection::Center,
                        "Half the thickness inside, half outside",
                    ),
                ] {
                    ui.radio_value(&mut d.def.direction, dir, dir.label())
                        .on_hover_text(tip);
                }
            });
            ui.end_row();
            field_row(ui, &mut fx, "Thickness", &snapshot, "thickness");
        });
        let mut ev = super::command_ui::RowEvents::default();
        psection(ui, "shell_input", "Input Geometry", |ui| {
            let flow = self
                .command_flow()
                .unwrap_or_else(|| crate::command::flow::Flow::new(Vec::new(), None));
            let text = if d.def.remove.is_empty() {
                String::new()
            } else {
                format!("{} face(s)", d.def.remove.len())
            };
            ev = selector_rows(ui, &flow, &[("remove", text)]);
            plabel(ui, "Face chain");
            ui.checkbox(&mut d.def.chain, "Automatic Face Chain")
                .on_hover_text(
                    "A click also takes the faces tangent to the picked one (its rounds)",
                );
            ui.end_row();
            if bodies.len() > 1 {
                plabel(ui, "Solid");
                let current = d.def.solids.first().cloned();
                let shown = current
                    .as_ref()
                    .and_then(|c| bodies.iter().find(|(id, _)| id == c))
                    .map_or("Of the faces".to_owned(), |(_, n)| n.clone());
                egui::ComboBox::from_id_salt("shell_solid")
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(current.is_none(), "Of the faces")
                            .clicked()
                        {
                            d.def.solids.clear();
                        }
                        for (id, n) in &bodies {
                            if ui
                                .selectable_label(current.as_deref() == Some(id.as_str()), n)
                                .clicked()
                            {
                                d.def.solids = vec![id.clone()];
                            }
                        }
                    });
                ui.end_row();
            }
        });
        if ev.clear == Some("remove") {
            d.def.remove.clear();
        }
        // Advanced Properties: the exact kernel offsets exactly, so there is
        // nothing to approximate; shown for parity, disabled.
        egui::CollapsingHeader::new("Advanced Properties")
            .id_salt("shell_advanced")
            .default_open(false)
            .show(ui, |ui| {
                let why = "Not needed: the exact geometry kernel always keeps the thickness exactly, it never approximates (docs/spec/2.6-shell.md)";
                ui.add_enabled_ui(false, |ui| {
                    let mut allow = false;
                    ui.checkbox(&mut allow, "Allow Approximation")
                        .on_disabled_hover_text(why);
                    for m in ["Mean", "Never Too Thin", "Never Too Thick"] {
                        ui.radio(m == "Mean", m).on_disabled_hover_text(why);
                    }
                    ui.horizontal(|ui| {
                        ui.radio(true, "Optimized").on_disabled_hover_text(why);
                        ui.radio(false, "Specified").on_disabled_hover_text(why);
                    });
                });
            });
        let commits = std::mem::take(&mut fx.commits);
        drop(fx);
        for (field, text) in commits {
            let mut f = Feature::Shell(d.def.clone());
            match self.part().apply_draft_field(&mut f, field, &text) {
                Ok(()) => {
                    if let Feature::Shell(s) = f {
                        d.def = s;
                    }
                    self.file_status = None;
                }
                Err(e) => self.file_status = Some(Err(format!("Shell: {e}"))),
            }
        }
        self.shell_draft = Some(d.clone());
        let preview = self.shell_preview();
        match &preview {
            Err(e) if e == super::PREVIEW_PENDING => {
                ui.label(e);
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(50));
            }
            Err(e) => warning_box(ui, e),
            _ => {}
        }
        ui.horizontal(|ui| {
            let ok = preview.is_ok() || matches!(&preview, Err(e) if e == super::PREVIEW_PENDING);
            if ui
                .add_enabled(ok, egui::Button::new("OK").min_size(egui::vec2(60.0, 22.0)))
                .on_hover_text("Ctrl+Enter or Enter")
                .clicked()
                || super::keys::panel_ok_key(ui, ok)
            {
                self.commit_shell(false);
            }
            if d.edit.is_none()
                && ui
                    .add_enabled(
                        ok,
                        egui::Button::new("Apply").min_size(egui::vec2(60.0, 22.0)),
                    )
                    .on_hover_text("Create it and start a new shell with the same settings")
                    .clicked()
            {
                self.commit_shell(true);
            }
            if ui
                .add(egui::Button::new("Cancel").min_size(egui::vec2(60.0, 22.0)))
                .on_hover_text("Esc")
                .clicked()
            {
                self.shell_draft = None;
                self.end_feature_edit();
            }
        });
    }

    /// The Face Draft command's panel (`docs/spec/2.8-draft.md`): type,
    /// selectors (`gui/command_ui.rs`), extent and angles, options, Move
    /// Parting Line; preview, OK / Apply / Cancel.
    fn draft_dialog(&mut self, ui: &mut egui::Ui) {
        use super::command_ui::{RowEvents, selector_rows};
        use crate::part::{DraftExtent, DraftType, PartingSide, PartingTool, PullRef};
        let Some(mut d) = self.draft_draft.clone() else {
            return;
        };
        self.panel_header(ui, Icon::Draft, "Face Draft");
        self.preset_row(ui, "Face Draft", &mut d.def);
        let describe = |p: &crate::part::PlaneRef| match p {
            crate::part::PlaneRef::Origin(o) => format!("{o:?} Plane"),
            crate::part::PlaneRef::Face(n) => n.clone(),
            crate::part::PlaneRef::Work(id) => id.clone(),
            crate::part::PlaneRef::Context(id) => self
                .part()
                .context_ref(id)
                .map_or_else(|| id.clone(), |c| c.source.label.clone()),
            crate::part::PlaneRef::Fixed => "fixed".into(),
        };
        let line = |l: &crate::part::LineRef| match l {
            crate::part::LineRef::Origin(k) => format!("{} Axis", ["X", "Y", "Z"][(*k).min(2)]),
            crate::part::LineRef::Edge(n) => n.clone(),
            crate::part::LineRef::Work(id) => id.clone(),
        };
        psection(ui, "draft_type", "Draft Type", |ui| {
            plabel(ui, "Type");
            ui.vertical(|ui| {
                for (t, tip) in [
                    (
                        DraftType::FixedEdge,
                        "Each face turns about its edge nearest the click",
                    ),
                    (
                        DraftType::FixedPlane,
                        "The faces turn about their line on a plane; the pull is its normal",
                    ),
                    (
                        DraftType::PartingLine,
                        "The faces are drafted above and below a parting tool (a plane or a sketch line)",
                    ),
                ] {
                    if ui
                        .radio(d.def.draft_type == t, t.label())
                        .on_hover_text(tip)
                        .clicked()
                    {
                        d.set_type(t);
                    }
                }
            });
            ui.end_row();
        });
        let sketches: Vec<(String, String)> = self
            .part()
            .sketches
            .iter()
            .map(|s| (s.id.clone(), s.name.clone()))
            .collect();
        let work_axes: Vec<String> = {
            let mut v: Vec<String> = self.part().work.axes.keys().cloned().collect();
            v.sort();
            v
        };
        let mut ev = RowEvents::default();
        let mut pick_sketch: Option<String> = None;
        let mut pick_axis: Option<crate::part::LineRef> = None;
        psection(ui, "draft_input", "Input Geometry", |ui| {
            let flow = d.flow();
            let texts = vec![
                (
                    "pull",
                    match &d.def.pull {
                        Some(PullRef::Plane(p)) => describe(p),
                        Some(PullRef::Line(l)) => line(l),
                        None => String::new(),
                    },
                ),
                (
                    "plane",
                    d.def.plane.as_ref().map_or(String::new(), describe),
                ),
                (
                    "parting",
                    match &d.def.parting {
                        Some(PartingTool::Plane(p)) => describe(p),
                        Some(PartingTool::Sketch(id)) => sketches
                            .iter()
                            .find(|(i, _)| i == id)
                            .map_or(id.clone(), |(_, n)| n.clone()),
                        None => String::new(),
                    },
                ),
                (
                    "faces",
                    if d.def.faces.is_empty() {
                        String::new()
                    } else {
                        format!("{} face(s)", d.def.faces.len())
                    },
                ),
                (
                    "static",
                    if d.def.static_edges.is_empty() {
                        String::new()
                    } else {
                        format!("{} edge(s)", d.def.static_edges.len())
                    },
                ),
            ];
            ev = selector_rows(ui, &flow, &texts);
            if d.def.draft_type.has_pull() {
                plabel(ui, "");
                ui.horizontal(|ui| {
                    if ui
                        .selectable_label(d.def.flip, "Flip")
                        .on_hover_text("Reverse the pull direction")
                        .clicked()
                    {
                        d.def.flip = !d.def.flip;
                    }
                    for (k, n) in ["X", "Y", "Z"].iter().enumerate() {
                        if ui
                            .small_button(*n)
                            .on_hover_text("The origin axis as the pull direction")
                            .clicked()
                        {
                            pick_axis = Some(crate::part::LineRef::Origin(k));
                        }
                    }
                    for id in &work_axes {
                        if ui.small_button(id).on_hover_text("A work axis").clicked() {
                            pick_axis = Some(crate::part::LineRef::Work(id.clone()));
                        }
                    }
                });
                ui.end_row();
            } else {
                plabel(ui, "");
                if ui
                    .selectable_label(d.def.flip, "Flip")
                    .on_hover_text("Reverse the pull direction (the plane's normal)")
                    .clicked()
                {
                    d.def.flip = !d.def.flip;
                }
                ui.end_row();
            }
            if d.def.draft_type == DraftType::PartingLine && !sketches.is_empty() {
                plabel(ui, "Sketch");
                let current = d.def.parting_sketch().map(str::to_owned);
                let shown = current
                    .as_ref()
                    .and_then(|c| sketches.iter().find(|(i, _)| i == c))
                    .map_or("a plane in the view".to_owned(), |(_, n)| n.clone());
                egui::ComboBox::from_id_salt("draft_parting_sketch")
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        for (id, n) in &sketches {
                            if ui
                                .selectable_label(current.as_deref() == Some(id.as_str()), n)
                                .clicked()
                            {
                                pick_sketch = Some(id.clone());
                            }
                        }
                    })
                    .response
                    .on_hover_text("A 2D sketch whose lines are the parting line");
                ui.end_row();
            }
            plabel(ui, "Face chain");
            ui.checkbox(&mut d.def.chain, "Automatic Face Chain")
                .on_hover_text("A click also takes the faces tangent to the picked one");
            ui.end_row();
            plabel(ui, "Blending");
            ui.checkbox(&mut d.def.blending, "Automatic Blending").on_hover_text(
                "Rounds along the pull next to a drafted face are drafted with it and stay tangent",
            );
            ui.end_row();
        });
        if let Some(k) = ev.activate {
            d.set_active(k);
        }
        if let Some(k) = ev.clear {
            match k {
                "pull" => d.def.pull = None,
                "plane" => d.def.plane = None,
                "parting" => d.def.parting = None,
                "faces" => d.def.faces.clear(),
                "static" => d.def.static_edges.clear(),
                _ => {}
            }
            d.set_active(k);
        }
        if let Some(l) = pick_axis {
            d.def.pull = Some(PullRef::Line(l));
            if d.input == super::draft::DraftInput::Pull {
                let mut f = d.flow();
                if f.picked()
                    && let Some(k) = f.active_key()
                {
                    d.set_active(k);
                }
            }
        }
        let table = self.part().parameter_table();
        let mut fx = Fields {
            table: &table,
            key: "draft_draft".into(),
            commits: Vec::new(),
            bodies: Vec::new(),
        };
        let snapshot = Feature::Draft(d.def.clone());
        psection(ui, "draft_angle", "Angle", |ui| {
            let extents = d.def.draft_type.extents();
            if extents.len() > 1 {
                plabel(ui, "Extent");
                ui.horizontal(|ui| {
                    for e in extents {
                        let tip = match e {
                            DraftExtent::OneWay => "The whole face, in the pull direction",
                            DraftExtent::Symmetric => {
                                "Above and below the plane, the same angle, each away from it"
                            }
                            DraftExtent::Asymmetric => {
                                "Above (Draft Angle) and below (Draft Angle 2) the plane"
                            }
                        };
                        if ui
                            .selectable_label(d.def.effective_extent() == *e, e.label())
                            .on_hover_text(tip)
                            .clicked()
                        {
                            d.def.extent = *e;
                        }
                    }
                });
                ui.end_row();
            }
            field_row(ui, &mut fx, "Draft Angle", &snapshot, "angle");
            if super::draft::shows_angle2(d.def.effective_extent()) {
                field_row(ui, &mut fx, "Draft Angle 2", &snapshot, "angle2");
            }
        });
        let mut boundary = false;
        if d.def.draft_type == DraftType::PartingLine {
            psection(ui, "draft_move", "Move Parting Line", |ui| {
                plabel(ui, "Move");
                if ui
                    .checkbox(&mut d.def.move_parting, "Move Parting Line")
                    .on_hover_text(
                        "The faces pivot about static edges instead of the parting line, which moves",
                    )
                    .changed()
                    && d.def.move_parting
                {
                    d.input = super::draft::DraftInput::Static;
                }
                ui.end_row();
                if d.def.move_parting {
                    plabel(ui, "Angle for");
                    ui.horizontal(|ui| {
                        for s in [PartingSide::Top, PartingSide::Both, PartingSide::Bottom] {
                            if ui
                                .selectable_label(
                                    d.def.move_side == s,
                                    s.label().trim_start_matches("Angle for "),
                                )
                                .on_hover_text(s.label())
                                .clicked()
                            {
                                d.def.move_side = s;
                            }
                        }
                    });
                    ui.end_row();
                    plabel(ui, "Static Edges");
                    ui.horizontal(|ui| {
                        if ui
                            .button("Select Boundary")
                            .on_hover_text(
                                "The top or bottom edges of the faces, as Angle for says",
                            )
                            .clicked()
                        {
                            boundary = true;
                        }
                        if ui.button("Clear All").clicked() {
                            d.def.static_edges.clear();
                            d.input = super::draft::DraftInput::Static;
                        }
                    });
                    ui.end_row();
                }
            });
        }
        if boundary {
            let part = self.part();
            let lines = self.draft_sketch_lines();
            if let Err(e) = d
                .def
                .select_boundary(&part.body, &part.work, lines.as_deref())
            {
                self.file_status = Some(Err(format!("Face Draft: {e}")));
            }
        }
        let commits = std::mem::take(&mut fx.commits);
        drop(fx);
        for (field, text) in commits {
            let mut f = Feature::Draft(d.def.clone());
            match self.part().apply_draft_field(&mut f, field, &text) {
                Ok(()) => {
                    if let Feature::Draft(x) = f {
                        d.def = x;
                    }
                    self.file_status = None;
                }
                Err(e) => self.file_status = Some(Err(format!("Face Draft: {e}"))),
            }
        }
        self.draft_draft = Some(d.clone());
        if let Some(id) = pick_sketch {
            self.draft_parting_sketch(id);
            if let Some(nd) = &self.draft_draft {
                d = nd.clone();
            }
        }
        let preview = self.draft_preview();
        match &preview {
            Err(e) if e == super::PREVIEW_PENDING => {
                ui.label(e);
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(50));
            }
            Err(e) if !d.def.faces.is_empty() => warning_box(ui, e),
            _ => {}
        }
        ui.horizontal(|ui| {
            let ok = preview.is_ok() || matches!(&preview, Err(e) if e == super::PREVIEW_PENDING);
            if ui
                .add_enabled(ok, egui::Button::new("OK").min_size(egui::vec2(60.0, 22.0)))
                .on_hover_text("Ctrl+Enter; Enter when every input is there")
                .clicked()
                || super::keys::panel_ok_key(ui, ok)
            {
                self.commit_draft(false);
            }
            if d.edit.is_none()
                && ui
                    .add_enabled(
                        ok,
                        egui::Button::new("Apply").min_size(egui::vec2(60.0, 22.0)),
                    )
                    .on_hover_text("Create it and go on with new faces")
                    .clicked()
            {
                self.commit_draft(true);
            }
            if ui
                .add(egui::Button::new("Cancel").min_size(egui::vec2(60.0, 22.0)))
                .on_hover_text("Esc")
                .clicked()
            {
                self.draft_draft = None;
                self.end_feature_edit();
            }
        });
    }

    /// The Rectangular / Circular Pattern and Mirror panel
    /// (`docs/spec/2.11-patterns.md`, `docs/spec/2.12-mirror.md`).
    fn pattern_feature_dialog(&mut self, ui: &mut egui::Ui) {
        use super::pattern_feature::{PatternInput, describe_line, describe_plane, describe_pull};
        use crate::part::{Operation, PatternDirection, PatternKind, PullRef};
        let Some(mut d) = self.pattern_draft.clone() else {
            return;
        };
        self.panel_header(ui, super::pattern_feature::icon_of(&d.def), d.def.command());
        let selector = |ui: &mut egui::Ui, active: bool, text: String| {
            let t = if active {
                RichText::new(format!("> {text}")).strong().underline()
            } else {
                RichText::new(text)
            };
            ui.selectable_label(active, t).clicked()
        };
        let names: Vec<String> = d
            .def
            .features
            .iter()
            .map(|id| {
                self.part()
                    .features
                    .iter()
                    .find(|f| f.id() == id)
                    .map_or_else(|| format!("{id} (missing)"), |f| f.name().to_owned())
            })
            .collect();
        let work_axes: Vec<String> = {
            let mut v: Vec<String> = self.part().work.axes.keys().cloned().collect();
            v.sort();
            v
        };
        let input = d.input;
        let mut new_input = None;
        let mirror = matches!(d.def.kind, PatternKind::Mirror { .. });
        psection(ui, "pat_input", "Input Geometry", |ui| {
            plabel(ui, "Copy");
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(!d.def.solid_mode, "Features")
                    .on_hover_text("Copy individual features (Extrude, Revolve, patterns)")
                    .clicked()
                {
                    d.def.solid_mode = false;
                    new_input = Some(PatternInput::Geometry);
                }
                if ui
                    .selectable_label(d.def.solid_mode, "Solids")
                    .on_hover_text("Copy whole solids, with every feature on them")
                    .clicked()
                {
                    d.def.solid_mode = true;
                    new_input = Some(PatternInput::Geometry);
                }
            });
            ui.end_row();
            if d.def.solid_mode {
                plabel(ui, "Solids");
                let text = if d.def.solids.is_empty() {
                    "all (click one to choose)".to_owned()
                } else {
                    d.def.solids.join(", ")
                };
                if selector(ui, input == PatternInput::Geometry, text) {
                    new_input = Some(PatternInput::Geometry);
                }
            } else {
                plabel(ui, "Features");
                let text = if names.is_empty() {
                    "click a face or a browser row".to_owned()
                } else {
                    names.join(", ")
                };
                if selector(ui, input == PatternInput::Geometry, text) {
                    new_input = Some(PatternInput::Geometry);
                }
            }
            ui.end_row();
        });
        let table = self.part().parameter_table();
        let mut fx = Fields {
            table: &table,
            key: "pattern_draft".into(),
            commits: Vec::new(),
            bodies: Vec::new(),
        };
        let snapshot = Feature::Pattern(d.def.clone());
        let axis_buttons = |ui: &mut egui::Ui| -> Option<crate::part::LineRef> {
            let mut out = None;
            plabel(ui, "Origin axis");
            ui.horizontal(|ui| {
                for (k, n) in ["X", "Y", "Z"].iter().enumerate() {
                    if ui.small_button(*n).clicked() {
                        out = Some(crate::part::LineRef::Origin(k));
                    }
                }
                for id in &work_axes {
                    if ui.small_button(id).on_hover_text("A work axis").clicked() {
                        out = Some(crate::part::LineRef::Work(id.clone()));
                    }
                }
            });
            ui.end_row();
            out
        };
        match &mut d.def.kind {
            PatternKind::Rectangular { first, second } => {
                psection(ui, "pat_dir1", "Direction 1", |ui| {
                    plabel(ui, "Direction");
                    let text = first
                        .dir
                        .as_ref()
                        .map_or("click an edge or a plane".into(), describe_pull);
                    if selector(ui, input == PatternInput::Dir1, text) {
                        new_input = Some(PatternInput::Dir1);
                    }
                    ui.end_row();
                    if let Some(l) = axis_buttons(ui) {
                        first.dir = Some(PullRef::Line(l));
                    }
                    pattern_direction_rows(ui, first, &mut fx, &snapshot, "count1", "spacing1");
                });
                psection(ui, "pat_dir2", "Direction 2", |ui| {
                    plabel(ui, "Use");
                    let mut on = second.is_some();
                    if ui
                        .checkbox(&mut on, "")
                        .on_hover_text("Rows along a second direction")
                        .changed()
                    {
                        *second = on.then(PatternDirection::new);
                        new_input = on.then_some(PatternInput::Dir2);
                    }
                    ui.end_row();
                    if let Some(s) = second {
                        plabel(ui, "Direction");
                        let text = s
                            .dir
                            .as_ref()
                            .map_or("click an edge or a plane".into(), describe_pull);
                        if selector(ui, input == PatternInput::Dir2, text) {
                            new_input = Some(PatternInput::Dir2);
                        }
                        ui.end_row();
                        if let Some(l) = axis_buttons(ui) {
                            s.dir = Some(PullRef::Line(l));
                        }
                        pattern_direction_rows(ui, s, &mut fx, &snapshot, "count2", "spacing2");
                    }
                });
            }
            PatternKind::Circular {
                axis,
                flip,
                fitted,
                midplane,
                ..
            } => {
                psection(ui, "pat_axis", "Rotation Axis", |ui| {
                    plabel(ui, "Axis");
                    let text = axis
                        .as_ref()
                        .map_or("click a straight edge".into(), describe_line);
                    if selector(ui, input == PatternInput::Axis, text) {
                        new_input = Some(PatternInput::Axis);
                    }
                    ui.end_row();
                    if let Some(l) = axis_buttons(ui) {
                        *axis = Some(l);
                    }
                    plabel(ui, "Flip");
                    ui.checkbox(flip, "")
                        .on_hover_text("Turn the other way about the axis");
                    ui.end_row();
                });
                psection(ui, "pat_place", "Placement", |ui| {
                    field_row(ui, &mut fx, "Count", &snapshot, "count");
                    plabel(ui, "Distribution");
                    ui.horizontal(|ui| {
                        ui.selectable_value(fitted, false, "Incremental")
                            .on_hover_text("The angle between instances");
                        ui.selectable_value(fitted, true, "Fitted")
                            .on_hover_text("The whole angle (360° = a full circle)");
                    });
                    ui.end_row();
                    let label = if *fitted { "Total Angle" } else { "Angle" };
                    field_row(ui, &mut fx, label, &snapshot, "angle");
                    plabel(ui, "Midplane");
                    ui.checkbox(midplane, "")
                        .on_hover_text("Instances on both sides of the original");
                    ui.end_row();
                    plabel(ui, "Orientation");
                    ui.horizontal(|ui| {
                        ui.add_enabled(false, egui::RadioButton::new(true, "Rotational"));
                        ui.add_enabled(false, egui::RadioButton::new(false, "Fixed"))
                            .on_disabled_hover_text("Not yet: docs/spec/2.11-patterns.md");
                    });
                    ui.end_row();
                });
            }
            PatternKind::Mirror { plane } => {
                psection(ui, "pat_plane", "Mirror Plane", |ui| {
                    plabel(ui, "Plane");
                    let text = plane
                        .as_ref()
                        .map_or("click a plane or a planar face".into(), describe_plane);
                    if selector(ui, input == PatternInput::Plane, text) {
                        new_input = Some(PatternInput::Plane);
                    }
                    ui.end_row();
                    plabel(ui, "Origin plane");
                    ui.horizontal(|ui| {
                        for o in OriginPlane::ALL {
                            if ui.small_button(format!("{o:?}")).clicked() {
                                *plane = Some(PlaneRef::Origin(o));
                            }
                        }
                    });
                    ui.end_row();
                });
            }
        }
        if d.def.solid_mode {
            psection(ui, "pat_output", "Output", |ui| {
                plabel(ui, "Operation");
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut d.def.operation, Operation::Join, "Join")
                        .on_hover_text("The copies join the original solid");
                    ui.selectable_value(&mut d.def.operation, Operation::NewSolid, "New Solid")
                        .on_hover_text("Each copy is a solid of its own");
                });
                ui.end_row();
                if mirror {
                    plabel(ui, "Remove Original");
                    ui.checkbox(&mut d.def.remove_original, "")
                        .on_hover_text("Only the mirrored solid stays");
                    ui.end_row();
                }
            });
        }
        psection(ui, "pat_creation", "Creation", |ui| {
            plabel(ui, "Method");
            ui.vertical(|ui| {
                ui.add_enabled(false, egui::RadioButton::new(false, "Optimized"))
                    .on_disabled_hover_text("Not yet: every copy is built as Identical");
                ui.radio(true, "Identical")
                    .on_hover_text("Each copy is the original feature's result, moved");
                ui.add_enabled(false, egui::RadioButton::new(false, "Adjust"))
                    .on_disabled_hover_text("Not yet: docs/spec/2.11-patterns.md");
            });
            ui.end_row();
        });
        let commits = std::mem::take(&mut fx.commits);
        drop(fx);
        if let Some(i) = new_input {
            d.input = i;
        }
        let cmd = d.def.command();
        for (field, text) in commits {
            let mut f = Feature::Pattern(d.def.clone());
            match self.part().apply_draft_field(&mut f, field, &text) {
                Ok(()) => {
                    if let Feature::Pattern(x) = f {
                        d.def = x;
                    }
                    self.file_status = None;
                }
                Err(e) => self.file_status = Some(Err(format!("{cmd}: {e}"))),
            }
        }
        self.pattern_draft = Some(d.clone());
        let preview = self.pattern_preview();
        let has_input = !d.def.features.is_empty() || d.def.solid_mode;
        match &preview {
            Err(e) if e == super::PREVIEW_PENDING => {
                ui.label(e);
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(50));
            }
            Err(e) if has_input => warning_box(ui, e),
            _ => {}
        }
        ui.horizontal(|ui| {
            let ok = preview.is_ok() || matches!(&preview, Err(e) if e == super::PREVIEW_PENDING);
            if ui
                .add_enabled(ok, egui::Button::new("OK").min_size(egui::vec2(60.0, 22.0)))
                .on_hover_text("Enter, Ctrl+Enter")
                .clicked()
                || super::keys::panel_ok_key(ui, ok)
            {
                self.commit_pattern(false);
            }
            if d.edit.is_none()
                && ui
                    .add_enabled(
                        ok,
                        egui::Button::new("Apply").min_size(egui::vec2(60.0, 22.0)),
                    )
                    .on_hover_text("Create it and go on with new features")
                    .clicked()
            {
                self.commit_pattern(true);
            }
            if ui
                .add(egui::Button::new("Cancel").min_size(egui::vec2(60.0, 22.0)))
                .clicked()
            {
                self.pattern_draft = None;
                self.end_feature_edit();
            }
        });
    }

    /// Properties of a pattern / mirror: what it copies, its counts,
    /// spacings and angle with expressions, Flip.
    fn pattern_properties(&mut self, ui: &mut egui::Ui, k: usize, p: crate::part::PatternFeature) {
        use crate::part::PatternKind;
        self.panel_header(ui, super::pattern_feature::icon_of(&p), &p.name);
        if let Some(h) = self.part().feature_health.get(&p.id) {
            warning_box(ui, h);
        }
        let mut changed = p.clone();
        psection(ui, &format!("pat_in_{k}"), "Input", |ui| {
            plabel(ui, "Copies");
            ui.label(super::pattern_feature::summary(&p));
            ui.end_row();
            match &mut changed.kind {
                PatternKind::Rectangular { first, second } => {
                    plabel(ui, "Flip 1");
                    ui.checkbox(&mut first.flip, "");
                    ui.end_row();
                    if let Some(s) = second {
                        plabel(ui, "Flip 2");
                        ui.checkbox(&mut s.flip, "");
                        ui.end_row();
                    }
                }
                PatternKind::Circular { flip, .. } => {
                    plabel(ui, "Flip");
                    ui.checkbox(flip, "");
                    ui.end_row();
                }
                PatternKind::Mirror { plane } => {
                    plabel(ui, "Plane");
                    ui.label(
                        plane
                            .as_ref()
                            .map_or("none".into(), super::pattern_feature::describe_plane),
                    );
                    ui.end_row();
                }
            }
        });
        if changed != p {
            self.edit_feature(k, Feature::Pattern(changed));
            return;
        }
        let table = self.part().parameter_table();
        let mut fx = Fields {
            table: &table,
            key: format!("feature{k}"),
            commits: Vec::new(),
            bodies: Vec::new(),
        };
        let f = Feature::Pattern(p.clone());
        let fields: &[(&str, &'static str)] = match &p.kind {
            PatternKind::Rectangular {
                second: Some(_), ..
            } => &[
                ("Count 1", "count1"),
                ("Spacing 1", "spacing1"),
                ("Count 2", "count2"),
                ("Spacing 2", "spacing2"),
            ],
            PatternKind::Rectangular { .. } => &[("Count", "count1"), ("Spacing", "spacing1")],
            PatternKind::Circular { .. } => &[("Count", "count"), ("Angle", "angle")],
            PatternKind::Mirror { .. } => &[],
        };
        if !fields.is_empty() {
            psection(ui, &format!("pat_val_{k}"), "Values", |ui| {
                for (label, field) in fields {
                    field_row(ui, &mut fx, label, &f, field);
                }
            });
        }
        let commits = std::mem::take(&mut fx.commits);
        drop(fx);
        for (field, text) in commits {
            self.checkpoint("Edit Feature");
            match self.part_mut().set_feature_field(k, field, &text) {
                Ok(()) => {
                    if let Err(e) = self.part_mut().regenerate() {
                        self.file_status = Some(Err(e.to_string()));
                    }
                    self.scene_dirty = true;
                }
                Err(e) => {
                    self.history.cancel();
                    self.file_status = Some(Err(format!("Edit Feature: {e}")));
                }
            }
        }
        if ui.button("Edit Feature").clicked() {
            self.edit_feature_dialog(k);
        }
        if ui.button("Delete").clicked() {
            self.delete_feature(k);
        }
    }

    /// Properties of a Face Draft: type, faces, angle with expressions.
    fn draft_properties(&mut self, ui: &mut egui::Ui, k: usize, dr: crate::part::DraftFeature) {
        self.panel_header(ui, Icon::Draft, &dr.name);
        if let Some(h) = self.part().feature_health.get(&dr.id) {
            warning_box(ui, h);
        }
        let mut flip_change = None;
        psection(ui, &format!("draft_in_{k}"), "Input", |ui| {
            plabel(ui, "Type");
            ui.label(dr.draft_type.label());
            ui.end_row();
            plabel(ui, "Faces");
            ui.label(format!("{}", dr.faces.len()));
            ui.end_row();
            plabel(ui, "Flip");
            let mut f = dr.flip;
            if ui.checkbox(&mut f, "").changed() {
                flip_change = Some(f);
            }
            ui.end_row();
        });
        if let Some(f) = flip_change {
            let mut nd = dr.clone();
            nd.flip = f;
            self.edit_feature(k, Feature::Draft(nd));
            return;
        }
        let table = self.part().parameter_table();
        let mut fx = Fields {
            table: &table,
            key: format!("feature{k}"),
            commits: Vec::new(),
            bodies: Vec::new(),
        };
        let f = Feature::Draft(dr.clone());
        psection(ui, &format!("draft_val_{k}"), "Angle", |ui| {
            field_row(ui, &mut fx, "Draft Angle", &f, "angle");
        });
        let commits = std::mem::take(&mut fx.commits);
        drop(fx);
        for (field, text) in commits {
            self.checkpoint("Edit Feature");
            match self.part_mut().set_feature_field(k, field, &text) {
                Ok(()) => {
                    if let Err(e) = self.part_mut().regenerate() {
                        self.file_status = Some(Err(e.to_string()));
                    }
                    self.scene_dirty = true;
                }
                Err(e) => {
                    self.history.cancel();
                    self.file_status = Some(Err(format!("Edit Feature: {e}")));
                }
            }
        }
        if ui.button("Edit Feature").clicked() {
            self.edit_feature_dialog(k);
        }
        if ui.button("Delete").clicked() {
            self.delete_feature(k);
        }
    }

    /// Properties of a Shell: direction, faces, thickness with expressions.
    fn shell_properties(&mut self, ui: &mut egui::Ui, k: usize, s: crate::part::ShellFeature) {
        use crate::part::ShellDirection;
        self.panel_header(ui, Icon::Shell, &s.name);
        if let Some(h) = self.part().feature_health.get(&s.id) {
            warning_box(ui, h);
        }
        let mut dir_change = None;
        psection(ui, &format!("shell_in_{k}"), "Input", |ui| {
            plabel(ui, "Remove Faces");
            ui.label(format!("{}", s.remove.len())).on_hover_text(
                s.remove
                    .iter()
                    .map(|r| r.face.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
            ui.end_row();
            plabel(ui, "Direction");
            let mut dir = s.direction;
            ui.vertical(|ui| {
                for d in [
                    ShellDirection::Inside,
                    ShellDirection::Outside,
                    ShellDirection::Center,
                ] {
                    ui.radio_value(&mut dir, d, d.label());
                }
            });
            if dir != s.direction {
                dir_change = Some(dir);
            }
            ui.end_row();
        });
        if let Some(dir) = dir_change {
            let mut ns = s.clone();
            ns.direction = dir;
            self.edit_feature(k, Feature::Shell(ns));
            return;
        }
        let table = self.part().parameter_table();
        let mut fx = Fields {
            table: &table,
            key: format!("feature{k}"),
            commits: Vec::new(),
            bodies: Vec::new(),
        };
        let f = Feature::Shell(s.clone());
        psection(ui, &format!("shell_val_{k}"), "Size", |ui| {
            field_row(ui, &mut fx, "Thickness", &f, "thickness");
        });
        let commits = std::mem::take(&mut fx.commits);
        drop(fx);
        for (field, text) in commits {
            self.checkpoint("Edit Feature");
            match self.part_mut().set_feature_field(k, field, &text) {
                Ok(()) => {
                    if let Err(e) = self.part_mut().regenerate() {
                        self.file_status = Some(Err(e.to_string()));
                    }
                    self.scene_dirty = true;
                }
                Err(e) => {
                    self.history.cancel();
                    self.file_status = Some(Err(format!("Edit Feature: {e}")));
                }
            }
        }
        if ui.button("Edit Feature").clicked() {
            self.edit_feature_dialog(k);
        }
        if ui.button("Delete").clicked() {
            self.delete_feature(k);
        }
    }

    /// Properties of a Fillet / Chamfer: edges, values with expressions.
    fn blend_properties(&mut self, ui: &mut egui::Ui, k: usize, b: crate::part::BlendFeature) {
        use crate::part::{BlendKind, ChamferMethod};
        let icon = match b.kind {
            BlendKind::Fillet { .. } => Icon::FilletEdge,
            BlendKind::Chamfer { .. } => Icon::ChamferEdge,
        };
        self.panel_header(ui, icon, &b.name);
        if let Some(h) = self.part().feature_health.get(&b.id) {
            warning_box(ui, h);
        }
        let mut method_change = None;
        let mut cone_change = None;
        psection(ui, &format!("blend_in_{k}"), "Input", |ui| {
            if matches!(b.kind, BlendKind::Fillet { .. }) {
                plabel(ui, "Type");
                ui.label(match b.fillet_type {
                    crate::part::FilletType::Constant if !b.sets.is_empty() => {
                        format!("Constant, {} sets", b.sets.len() + 1)
                    }
                    crate::part::FilletType::Constant => "Constant".into(),
                    crate::part::FilletType::Variable => {
                        if b.smooth {
                            "Variable (smooth)".into()
                        } else {
                            "Variable".into()
                        }
                    }
                    crate::part::FilletType::Chord => "Chord".into(),
                    crate::part::FilletType::FullRound => "Full Round".into(),
                });
                ui.end_row();
            }
            match &b.full_round {
                Some(face) => {
                    plabel(ui, "Middle face");
                    ui.label(face.clone());
                }
                None => {
                    plabel(ui, "Edges");
                    ui.label(format!("{}", b.all_edges().len()));
                }
            }
            ui.end_row();
            if let BlendKind::Chamfer { method, flip, .. } = b.kind {
                plabel(ui, "Method");
                let mut m = method;
                let mut fl = flip;
                ui.vertical(|ui| {
                    ui.radio_value(&mut m, ChamferMethod::Distance, "Distance");
                    ui.radio_value(&mut m, ChamferMethod::TwoDistances, "Two Distances");
                    ui.radio_value(&mut m, ChamferMethod::DistanceAngle, "Distance and Angle");
                    if m != ChamferMethod::Distance {
                        ui.checkbox(&mut fl, "Flip");
                    }
                });
                if (m, fl) != (method, flip) {
                    method_change = Some((m, fl));
                }
                ui.end_row();
                plabel(ui, "Corners");
                let mut cone = b.cone_corners;
                if ui
                    .checkbox(&mut cone, "Cone at small rounds")
                    .on_hover_text("Where the chamfer is larger than a round of the loop: follow the round as a cone up to its tip. Off: the flat bevels run on through the corner and cut the round.")
                    .changed()
                {
                    cone_change = Some(cone);
                }
                ui.end_row();
            }
        });
        if let Some((m, fl)) = method_change {
            let mut nb = b.clone();
            if let BlendKind::Chamfer { method, flip, .. } = &mut nb.kind {
                *method = m;
                *flip = fl;
            }
            self.edit_feature(k, Feature::Blend(nb));
            return;
        }
        if let Some(cone) = cone_change {
            let mut nb = b.clone();
            nb.cone_corners = cone;
            self.edit_feature(k, Feature::Blend(nb));
            return;
        }
        let table = self.part().parameter_table();
        let mut fx = Fields {
            table: &table,
            key: format!("feature{k}"),
            commits: Vec::new(),
            bodies: Vec::new(),
        };
        let f = Feature::Blend(b.clone());
        psection(ui, &format!("blend_val_{k}"), "Size", |ui| {
            for field in crate::part::params::feature_field_names(&f) {
                let label = match field {
                    "radius" => "Radius",
                    "distance" => "Distance",
                    "distance2" => "Distance 2",
                    "angle" => "Angle",
                    "to_start" => "To Start",
                    "partial_length" => "Chamfer length",
                    other => other,
                };
                field_row(ui, &mut fx, label, &f, field);
            }
        });
        let commits = std::mem::take(&mut fx.commits);
        drop(fx);
        for (field, text) in commits {
            self.checkpoint("Edit Feature");
            match self.part_mut().set_feature_field(k, field, &text) {
                Ok(()) => {
                    if let Err(e) = self.part_mut().regenerate() {
                        self.file_status = Some(Err(e.to_string()));
                    }
                    self.scene_dirty = true;
                }
                Err(e) => {
                    self.history.cancel();
                    self.file_status = Some(Err(format!("Edit Feature: {e}")));
                }
            }
        }
        if ui.button("Delete").clicked() {
            self.delete_feature(k);
        }
    }

    fn sketch_properties(&mut self, ui: &mut egui::Ui, i: usize) {
        let editing = self.editing == Some(i);
        let ps = self.part().sketches[i].clone();

        let mut name = ps.name.clone();
        if ui.text_edit_singleline(&mut name).changed() {
            self.checkpoint("Rename Sketch");
            self.part_mut().sketches[i].name = name;
        }
        ui.horizontal(|ui| {
            ui.label(format!("Plane: {}", ps.placement.describe()));
            if ui
                .small_button("Redefine")
                .on_hover_text("Move the sketch to another plane or face")
                .clicked()
            {
                self.browser_cmd(BrowserCmd::Redefine(i));
            }
        });
        if let Some(h) = &ps.health {
            ui.colored_label(WARNING, format!("Warning: {h}"));
        }
        ui.horizontal(|ui| {
            if editing {
                if ui.button("Finish Sketch").clicked() {
                    self.finish_sketch();
                }
            } else if ui.button("Edit Sketch").clicked() {
                self.edit_sketch(i);
            }
            if ui.button("Look At").clicked() {
                self.look_at_sketch(i);
            }
        });

        // ---- Edit Coordinate System ----
        ui.separator();
        ui.label(RichText::new("Coordinate System").strong());
        let body = &self.part().body;
        let face = match &ps.placement.plane {
            PlaneRef::Face(n) => body.find_face(n),
            _ => None,
        };
        let vertices: Vec<String> = match face {
            Some(f) => body.face_vertices(f),
            None => body
                .faces()
                .flat_map(|(f, _)| body.face_vertices(f))
                .collect(),
        }
        .into_iter()
        .filter_map(|v| body.vertex(v)?.name.clone())
        .fold(Vec::new(), |mut acc, n| {
            if !acc.contains(&n) {
                acc.push(n);
            }
            acc
        });
        let edges: Vec<String> = match face {
            Some(f) => body.face_edges(f),
            None => body.faces().flat_map(|(f, _)| body.face_edges(f)).collect(),
        }
        .into_iter()
        .filter(|&e| body.edge_direction(e).is_some())
        .filter_map(|e| body.edge(e)?.name.clone())
        .fold(Vec::new(), |mut acc, n| {
            if !acc.contains(&n) {
                acc.push(n);
            }
            acc
        });

        let mut origin = ps.placement.origin.clone();
        let mut axis = ps.placement.axis_edge.clone();
        let mut axis_is_x = ps.placement.axis_is_x;
        let mut natural = ps.placement.natural_direction;
        let origin_text = |o: &PointRef| match o {
            PointRef::PartOrigin => "Part origin (projected)".to_owned(),
            PointRef::Vertex(n) => n.clone(),
            other => other.describe(),
        };
        // The shared label column (a plain grid cut "Axis edge" to "Axis …").
        egui::Grid::new("csys")
            .num_columns(2)
            .min_col_width(label_width(ui.ctx()))
            .show(ui, |ui| {
                plabel(ui, "Origin");
                egui::ComboBox::from_id_salt("csys_origin")
                    .selected_text(origin_text(&origin))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut origin,
                            PointRef::PartOrigin,
                            origin_text(&PointRef::PartOrigin),
                        );
                        for v in &vertices {
                            ui.selectable_value(&mut origin, PointRef::Vertex(v.clone()), v);
                        }
                    });
                ui.end_row();
                plabel(ui, "Axis edge");
                egui::ComboBox::from_id_salt("csys_axis")
                    .selected_text(axis.clone().unwrap_or_else(|| "Default".into()))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut axis, None, "Default");
                        for e in &edges {
                            ui.selectable_value(&mut axis, Some(e.clone()), e);
                        }
                    });
                ui.end_row();
            });
        ui.add_enabled_ui(axis.is_some(), |ui| {
            let mut is_y = !axis_is_x;
            ui.checkbox(&mut is_y, "Edge defines the Y axis");
            axis_is_x = !is_y;
        });
        let mut flip = !natural;
        ui.checkbox(&mut flip, "Flip axis direction");
        natural = !flip;
        if (origin.clone(), axis.clone(), axis_is_x, natural)
            != (
                ps.placement.origin.clone(),
                ps.placement.axis_edge.clone(),
                ps.placement.axis_is_x,
                ps.placement.natural_direction,
            )
        {
            self.checkpoint("Edit Coordinate System");
            if let Err(e) = self
                .part_mut()
                .set_coordinate_system(i, origin, axis, axis_is_x, natural)
            {
                self.file_status = Some(Err(e.to_string()));
            }
            self.solve();
        }

        // References: projected geometry and where it comes from.
        let refs = ps.references(&self.part().body, &self.part().work);
        let missing = refs.iter().filter(|r| !r.ok).count();
        let title = if missing > 0 {
            format!("References ({}, {missing} missing)", refs.len())
        } else {
            format!("References ({})", refs.len())
        };
        let mut action: Option<(super::project::RefAction, crate::part::RefTarget)> = None;
        let relinking = self.relink.filter(|&(si, _)| si == i).map(|(_, k)| k);
        egui::CollapsingHeader::new(RichText::new(title).strong())
            .id_salt(("sketch_refs", i))
            .default_open(missing > 0)
            .show(ui, |ui| {
                if refs.is_empty() {
                    ui.label(RichText::new("No projected geometry").small().weak());
                }
                let mut edge_no = std::collections::HashMap::new();
                for r in &refs {
                    // Short label: the source feature and a number; the
                    // full persistent names in the tooltip.
                    let feature = r
                        .source
                        .split_once(' ')
                        .map_or("", |(_, rest)| rest)
                        .split(':')
                        .next()
                        .unwrap_or_default()
                        .to_owned();
                    let short = match (&r.label, r.target) {
                        (Some(l), _) => l.clone(),
                        (None, crate::part::RefTarget::Line(_)) => {
                            let n = edge_no.entry(feature.clone()).or_insert(0);
                            *n += 1;
                            format!("{feature} · edge {n}")
                        }
                        (None, crate::part::RefTarget::Point(_)) if r.source.starts_with("Origin") => {
                            "Origin · Center Point".to_owned()
                        }
                        (None, crate::part::RefTarget::Point(_)) => format!("{feature} · vertex"),
                        (None, crate::part::RefTarget::Curve(_)) => format!("{feature} · curve"),
                    };
                    let link = ps.link_of(r.target);
                    let row = ui.horizontal(|ui| {
                        if r.ok {
                            ui.colored_label(Color32::from_rgb(60, 170, 80), "✔");
                        } else {
                            ui.colored_label(Color32::from_rgb(230, 140, 0), "!")
                                .on_hover_text("The source is missing: the geometry is frozen");
                        }
                        let text = if link.is_some() && link == relinking {
                            RichText::new(format!("{short} (pick the new source)")).strong()
                        } else {
                            RichText::new(short)
                        };
                        ui.label(text).on_hover_text(&r.source);
                        use super::project::RefAction;
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .small_button("x")
                                .on_hover_text(
                                    "Delete the projected geometry and the constraints that use it",
                                )
                                .clicked()
                            {
                                action = Some((RefAction::Delete, r.target));
                            }
                            if ui
                                .small_button("Break Link")
                                .on_hover_text(
                                    "Keep the geometry, no longer updated from its source",
                                )
                                .clicked()
                            {
                                action = Some((RefAction::BreakLink, r.target));
                            }
                            if link.is_some()
                                && ui
                                    .small_button("Replace")
                                    .on_hover_text(
                                        "Replace Reference: click the new source in the view; the constraints stay",
                                    )
                                    .clicked()
                            {
                                action = Some((RefAction::Replace, r.target));
                            }
                            if matches!(
                                r.target,
                                crate::part::RefTarget::Line(_) | crate::part::RefTarget::Curve(_)
                            ) && editing
                                && ui
                                    .small_button("Select")
                                    .on_hover_text("Select the projected curve")
                                    .clicked()
                            {
                                action = Some((RefAction::Select, r.target));
                            }
                        });
                    });
                    if row.response.contains_pointer() {
                        self.ref_hover = Some((i, r.points.clone()));
                    }
                }
            });
        if let Some((act, target)) = action {
            self.reference_action(i, target, act);
        }

        if editing {
            ui.separator();
            ui.label(RichText::new("Solver").strong());
            match &self.report {
                Some(Ok(r)) => {
                    ui.colored_label(self.sketch_color(), self.sketch_state_text());
                    ui.label(format!("DOF left: {}    redundant: {}", r.dof, r.redundant));
                    ui.label(format!(
                        "iterations: {}    max residual: {:.1e}",
                        r.iterations, r.max_residual
                    ));
                }
                Some(Err(e)) => {
                    ui.colored_label(Color32::RED, e);
                }
                None => {}
            }
            let sk = &self.part().sketches[i].sketch;
            for (n, (pid, _)) in sk.patterns().enumerate() {
                if let Some(h) = sk.pattern_health(pid) {
                    ui.colored_label(WARNING, format!("Warning: pattern {}: {h}", n + 1));
                }
            }
            self.constraint_list(ui, i);
        }

        ui.separator();
        if ui.button("Delete Sketch").clicked() {
            self.delete_sketch(i);
        }
    }

    fn constraint_list(&mut self, ui: &mut egui::Ui, i: usize) {
        if let Some(dof) = &self.dof {
            ui.separator();
            let free = dof
                .points
                .values()
                .filter(|f| **f != crate::sketch::PointFreedom::Fixed)
                .count();
            ui.label(format!(
                "Free points: {free} · rotating curves: {} · free radii: {}",
                dof.rotating.len(),
                dof.free_radius.len()
            ));
            ui.toggle_value(&mut self.show_dof, "Show All Degrees of Freedom");
        }
        if !self.tools.selected.is_empty()
            || !self.tools.selected_constraints.is_empty()
            || !self.tools.selected_points.is_empty()
        {
            ui.separator();
            ui.horizontal(|ui| {
                let points = match self.tools.selected_points.len() {
                    0 => String::new(),
                    n => format!("{n} point(s), "),
                };
                ui.label(format!(
                    "{points}{} curve(s), {} constraint(s) selected",
                    self.tools.selected.len(),
                    self.tools.selected_constraints.len()
                ));
                if ui.button("Delete").on_hover_text("Delete key").clicked() {
                    self.delete_selection();
                }
            });
        }
        ui.separator();
        ui.label(RichText::new("Constraints").strong());
        let mut value_changes = Vec::new();
        let mut driven_changes = Vec::new();
        let mut removals = Vec::new();
        let mut hovered_row = None;
        let mut toggled = None;
        let selected = self.tools.selected_constraints.clone();
        let sketch = &self.part().sketches[i].sketch;
        let rows: Vec<_> = sketch
            .constraints()
            .map(|(id, c)| (id, c.clone(), sketch.is_driven(id), sketch.measure(id)))
            .collect();
        for (id, c, driven, measured) in rows {
            ui.horizontal(|ui| {
                if ui
                    .small_button("x")
                    .on_hover_text("Delete constraint")
                    .clicked()
                {
                    removals.push(id);
                }
                // Hover highlights the constrained geometry in the window;
                // click selects (Delete removes, as on the canvas).
                let r = ui.selectable_label(selected.contains(&id), c.name());
                if r.hovered() {
                    hovered_row = Some(id);
                }
                if r.clicked() {
                    toggled = Some(id);
                }
                let Some(value) = c.value() else { return };
                let is_angle = matches!(c, Constraint::Angle { .. });
                if driven {
                    // Reference dimension: shows the measured value only.
                    let v = measured.unwrap_or(value);
                    let text = if is_angle {
                        format!("({:.2}°)", v.to_degrees().abs())
                    } else {
                        format!("({v:.3} mm)")
                    };
                    ui.label(RichText::new(text).italics());
                } else if is_angle {
                    let mut deg = value.to_degrees().abs();
                    if ui
                        .add(
                            super::valuefield::ExprValue::new(&mut deg)
                                .speed(0.5)
                                .range(0.01..=180.0)
                                .suffix("°"),
                        )
                        .changed()
                    {
                        value_changes.push((
                            id,
                            deg.to_radians()
                                .copysign(if value < 0.0 { -1.0 } else { 1.0 }),
                        ));
                    }
                } else {
                    let mut v = value;
                    if ui
                        .add(
                            super::valuefield::ExprValue::new(&mut v)
                                .speed(0.1)
                                .range(0.001..=10000.0)
                                .suffix(" mm"),
                        )
                        .changed()
                    {
                        value_changes.push((id, v));
                    }
                }
                let mut d = driven;
                if ui
                    .checkbox(&mut d, "ref")
                    .on_hover_text("Driven (reference) dimension")
                    .changed()
                {
                    driven_changes.push((id, d));
                }
            });
        }
        self.list_hover = hovered_row;
        if let Some(id) = toggled {
            let sel = &mut self.tools.selected_constraints;
            if let Some(k) = sel.iter().position(|&x| x == id) {
                sel.remove(k);
            } else {
                sel.push(id);
            }
        }
        if !value_changes.is_empty() || !removals.is_empty() || !driven_changes.is_empty() {
            self.checkpoint("Edit Constraints");
            let sketch = &mut self.part_mut().sketches[i].sketch;
            for (id, v) in value_changes {
                let _ = sketch.set_dimension(id, v);
            }
            for (id, d) in driven_changes {
                let _ = sketch.set_driven(id, d);
            }
            for id in removals {
                let _ = sketch.remove_constraint(id);
            }
            self.solve();
        }
    }

    /// Human-readable state of the edited sketch, Inventor wording.
    pub(super) fn sketch_state_text(&self) -> String {
        match &self.report {
            Some(Ok(r)) if r.status == SolveStatus::Failed => {
                "Constraints conflict — sketch unchanged".into()
            }
            Some(Ok(r)) if r.redundant > 0 => {
                format!("Over-constrained ({} redundant)", r.redundant)
            }
            Some(Ok(r)) if r.dof == 0 => "Fully constrained".into(),
            Some(Ok(r)) => format!("Needs {} dimension(s)/constraint(s)", r.dof),
            Some(Err(e)) => e.clone(),
            None => String::new(),
        }
    }

    /// Bottom status bar: prompt for the current mode, messages, units.
    pub(super) fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            // The right-hand controls first; the hints get the rest of the
            // width and are cut with "…" instead of running under them.
            let editing = self.editing.is_some();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(format!("Mouse: {}", self.mouse.label()));
                if let Some(note) = self.filter_override_note() {
                    ui.separator();
                    ui.label(RichText::new(note).color(egui::Color32::from_rgb(200, 120, 20)));
                }
                let mut bg = super::i18n::bulgarian();
                if ui
                    .toggle_value(&mut bg, "БГ")
                    .on_hover_text("Hints in Bulgarian (command names stay English)")
                    .changed()
                {
                    super::i18n::set_bulgarian(bg);
                }
                if editing {
                    ui.separator();
                    ui.toggle_value(&mut self.relax, "Relax")
                        .on_hover_text("Relax mode: dragging changes dimensions");
                    ui.toggle_value(&mut self.tools.inference.point_alignment, "Align")
                        .on_hover_text(
                            "Point alignment: snap new points level with existing ones \
                             (dotted lines). Ctrl suppresses it for one pick",
                        );
                    ui.toggle_value(&mut self.slice, "Slice").on_hover_text(
                        "Slice Graphics (F7): cut away the model in front of the sketch plane",
                    );
                    ui.toggle_value(&mut self.show_dof, "DOF")
                        .on_hover_text("Show All Degrees of Freedom");
                    ui.separator();
                    egui::ComboBox::from_id_salt("dim_display")
                        .selected_text(self.dim_display.label())
                        .show_ui(ui, |ui| {
                            for m in crate::sketch::DimDisplay::ALL {
                                ui.selectable_value(&mut self.dim_display, m, m.label());
                            }
                        })
                        .response
                        .on_hover_text(
                            "Dimension display: value, parameter name, equation or tolerance",
                        );
                }
                ui.separator();
                // The CPU work of a frame, smoothed (measured in `ui`): shows
                // when the view gets slow. Idle time between frames is not in it.
                let ms = ui
                    .ctx()
                    .memory(|m| m.data.get_temp::<f32>(egui::Id::new("frame_ms")))
                    .unwrap_or(0.0);
                ui.label(RichText::new(format!("{ms:.0} ms")).weak())
                    .on_hover_text("Time the program spends on a frame (without the GPU and the wait for input)");
                ui.separator();
                ui.label("ISO");
                ui.separator();
                ui.label("mm");
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    self.alias_buffer_label(ui);
                    match (self.mode, self.editing) {
                        _ if self.home.shown => {
                            let text = super::i18n::tr(
                                "Home: New or Open a file, or double-click a recent one",
                            );
                            ui.add(egui::Label::new(RichText::new(text).strong()).truncate());
                        }
                        (Mode::Normal, None) if self.sm.draft.is_some() => {
                            let prompt = self.sm_prompt().unwrap_or_default();
                            let text = super::i18n::tr(&prompt);
                            ui.add(egui::Label::new(RichText::new(text).strong()).truncate());
                        }
                        _ if self.is_drawing() => {
                            let prompt = self.drw_prompt();
                            let text = super::i18n::tr(&prompt);
                            ui.add(egui::Label::new(RichText::new(text).strong()).truncate());
                        }
                        _ if self.is_assembly() => {
                            let prompt = self.command_prompt().or_else(|| self.constrain_prompt()).unwrap_or_else(|| self.asm_prompt());
                            let text = super::i18n::tr(&prompt);
                            ui.add(egui::Label::new(RichText::new(text).strong()).truncate());
                        }
                        (Mode::PickPlane(PickPurpose::NewSketch), _) => {
                            ui.colored_label(
                                WARNING,
                                super::i18n::tr(
                                    "Select a planar face or an origin plane to sketch on (Esc cancels)",
                                ),
                            );
                        }
                        (Mode::PickPlane(PickPurpose::Primitive(k)), _) => {
                            ui.colored_label(
                                WARNING,
                                format!(
                                    "{}: select a planar face or an origin plane for its sketch (Esc cancels)",
                                    k.label()
                                ),
                            );
                        }
                        (Mode::PickPlane(PickPurpose::Redefine(i)), _) => {
                            let name = self.part().sketches.get(i).map_or("", |s| s.name.as_str());
                            ui.colored_label(
                                WARNING,
                                format!("Select the new plane for {name} (Esc cancels)"),
                            );
                        }
                        (Mode::Normal, Some(_)) if self.project_geometry => {
                            ui.colored_label(
                                WARNING,
                                super::i18n::tr(if self.relink.is_some() {
                                    "Replace Reference: click the new source (a point, curve or edge of the same kind; Esc cancels)"
                                } else {
                                    "Project Geometry: click points or curves of earlier sketches, or model edges, to project them (Shift: as construction; Esc or right-click > Done ends)"
                                }),
                            );
                        }
                        (Mode::Normal, Some(i)) => {
                            let name = &self.part().sketches[i].name;
                            ui.colored_label(
                                self.sketch_color(),
                                format!("{name}: {}", self.sketch_state_text()),
                            );
                            ui.separator();
                            let mode = if self.tools.construction {
                                " [construction]"
                            } else {
                                ""
                            };
                            let prompt = self.tools.prompt();
                            let prompt = super::i18n::tr(&prompt);
                            ui.add(
                        egui::Label::new(RichText::new(format!("{prompt}{mode}")).strong())
                            .truncate(),
                    );
                        }
                (Mode::Normal, None) if self.blend_draft.is_some() => {
                    let text = self
                        .blend_draft
                        .as_ref()
                        .map(|d| {
                            use super::blend::EdgeMode;
                            let cmd = d.def.command();
                            let n = d.def.all_edges().len();
                            if d.def.fillet_type == crate::part::FilletType::FullRound {
                                format!("{cmd}: click the middle face (between two parallel side faces), then OK")
                            } else {
                                match d.mode {
                                    EdgeMode::Loop => format!("{cmd}: click a face, its nearest loop is taken ({n} edges), then OK"),
                                    EdgeMode::Feature => format!("{cmd}: click a face of a feature, its edges against the others are taken ({n}), then OK"),
                                    EdgeMode::Solid => format!("{cmd}: All Edges / All Fillets / All Rounds in the panel ({n} edges), then OK"),
                                    EdgeMode::Edge => format!("{cmd}: click edges ({n} selected; a second click removes one), then OK"),
                                }
                            }
                        })
                        .unwrap_or_default();
                    ui.add(egui::Label::new(RichText::new(text).strong()).truncate());
                }
                (Mode::Normal, None) if self.feature_prompt().is_some() => {
                    let text = self.feature_prompt().unwrap_or_default();
                    ui.add(egui::Label::new(RichText::new(text).strong()).truncate());
                }
                (Mode::Normal, None) if self.mat.adjusting() => {
                    ui.add(egui::Label::new(RichText::new(super::i18n::tr(super::material::ADJUST_PROMPT)).strong()).truncate());
                }
                (Mode::Normal, None) if self.coil_rib_open() => {
                    let text = self.coil_rib_prompt().unwrap_or_default();
                    ui.add(egui::Label::new(RichText::new(text).strong()).truncate());
                }
                (Mode::Normal, None) if self.sweep_loft_open() => {
                    let text = self.sweep_loft_prompt().unwrap_or_default();
                    ui.add(egui::Label::new(RichText::new(text).strong()).truncate());
                }
                (Mode::Normal, None) if self.shell_draft.is_some() => {
                    let text = self.command_prompt().unwrap_or_default();
                    ui.add(egui::Label::new(RichText::new(text).strong()).truncate());
                }
                (Mode::Normal, None) if self.hole_draft.is_some() => {
                    let d = self.hole_draft.as_ref();
                    let text = if d.is_some_and(|d| d.dir_pick) {
                        "Hole: click a planar face (its normal) or a straight edge for the direction"
                    } else if d.is_some_and(|d| d.to_pick) {
                        "Hole: click the face where the hole ends (planar or curved)"
                    } else if d.is_some_and(|d| d.edit.is_some()) {
                        "Hole: change the options, then OK (the centres are edited in the hole's sketch)"
                    } else if d.is_some_and(|d| d.count() == 0) {
                        "Hole: click a planar face, a sketch point or a circular edge for the centre"
                    } else {
                        "Hole: click more centres (a click on one removes it), straight edges to dimension the last one, then OK"
                    };
                    ui.add(egui::Label::new(RichText::new(super::i18n::tr(text)).strong()).truncate());
                }
                (Mode::Normal, None) if self.thread_draft.is_some() => {
                    let text = if self.thread_draft.as_ref().is_some_and(|d| d.face.is_none()) {
                        "Thread: click a cylindrical face near the end the thread starts at"
                    } else {
                        "Thread: set the size and the length, then OK (another click picks another face)"
                    };
                    ui.add(egui::Label::new(RichText::new(super::i18n::tr(text)).strong()).truncate());
                }
                (Mode::Normal, None) if self.combine_draft.is_some() || self.split_draft.is_some() => {
                    let text = self.command_prompt().unwrap_or_default();
                    ui.add(egui::Label::new(RichText::new(text).strong()).truncate());
                }
                (Mode::Normal, None) if self.section.as_ref().is_some_and(|s| s.editing) => {
                    let text = self.section_prompt().unwrap_or_default();
                    ui.add(egui::Label::new(RichText::new(text).strong()).truncate());
                }
                (Mode::Normal, None) if self.thicken_draft.is_some() => {
                    let text = self.thicken_prompt().unwrap_or_default();
                    ui.add(egui::Label::new(RichText::new(text).strong()).truncate());
                }
                (Mode::Normal, None) if self.delete_draft.is_some() => {
                    let text = self.delete_face_prompt().unwrap_or_default();
                    ui.add(egui::Label::new(RichText::new(text).strong()).truncate());
                }
                (Mode::Normal, None) if self.move_draft.is_some() => {
                    let text = self.move_bodies_prompt().unwrap_or_default();
                    ui.add(egui::Label::new(RichText::new(text).strong()).truncate());
                }
                (Mode::Normal, None) if self.part_planned.is_some() => {
                    let text = self.command_prompt().unwrap_or_default();
                    ui.add(egui::Label::new(RichText::new(text).strong()).truncate());
                }
                (Mode::Normal, None) if self.draft_draft.is_some() => {
                    let text = self.command_prompt().unwrap_or_default();
                    ui.add(egui::Label::new(RichText::new(text).strong()).truncate());
                }
                (Mode::Normal, None) if self.pattern_draft.is_some() => {
                    use super::pattern_feature::PatternInput;
                    let solid = self.pattern_draft.as_ref().is_some_and(|d| d.def.solid_mode);
                    let text = match self.pattern_draft.as_ref().map(|d| d.input) {
                        Some(PatternInput::Geometry) if solid => "Pattern: click the solids to copy (none: all), then the next selector",
                        Some(PatternInput::Geometry) => "Pattern: click a face of each feature to copy (or its browser row); a second click drops it",
                        Some(PatternInput::Dir1) => "Rectangular Pattern: click a straight edge or a plane for Direction 1, set count and spacing, then OK",
                        Some(PatternInput::Dir2) => "Rectangular Pattern: click a straight edge or a plane for Direction 2 (optional), then OK",
                        Some(PatternInput::Axis) => "Circular Pattern: click a straight edge or choose an axis, set count and angle, then OK",
                        _ => "Mirror: click the mirror plane (an origin or work plane, or a planar face), then OK",
                    };
                    ui.add(egui::Label::new(RichText::new(super::i18n::tr(text)).strong()).truncate());
                }
                (Mode::Normal, None) if self.work_draft.is_some() => {
                    let text = self
                        .work_draft
                        .as_ref()
                        .map(super::work::WorkDraft::prompt)
                        .unwrap_or_default();
                    ui.add(egui::Label::new(RichText::new(text).strong()).truncate());
                }
                        (Mode::Normal, None) => match self.selection_summary() {
                            Some(text) => {
                                ui.label(text);
                            }
                            None => {
                                ui.label(super::i18n::tr(
                                    "Ready — Start 2D Sketch (S), or right-click a face: New Sketch",
                                ));
                            }
                        },
                    }
                    ui.separator();
                    match &self.file_status {
                        Some(Err(e)) => {
                            ui.colored_label(Color32::RED, e);
                        }
                        Some(Ok(msg)) => {
                            ui.label(msg);
                        }
                        None => {
                            ui.add(
                        egui::Label::new(RichText::new(super::i18n::tr(self.nav_hint())).weak())
                            .truncate(),
                    );
                        }
                    }
                });
            });
        });
    }

    fn nav_hint(&self) -> &'static str {
        match self.mouse {
            MousePreset::Inventor => {
                "Shift+Middle / F4: orbit · Middle / F2: pan · Wheel / F3: zoom · F6: home"
            }
            MousePreset::Classic => {
                "Left: orbit · Right/Middle: pan · Wheel: zoom · Double-click: fit"
            }
        }
    }

    /// Tools → Application Options.
    pub(super) fn options_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_options;
        self.layout
            .place(
                "options",
                ctx.content_rect(),
                true,
                egui::Window::new("Application Options")
                    .open(&mut open)
                    .resizable(true)
                    .vscroll(true)
                    .collapsible(false),
            )
            .show(ctx, |ui| {
                ui.label(RichText::new("Navigation").strong());
                for preset in [MousePreset::Inventor, MousePreset::Classic] {
                    ui.radio_value(&mut self.mouse, preset, preset.label());
                }
                ui.checkbox(&mut self.invert_wheel, "Reverse zoom direction");
                ui.checkbox(&mut self.zoom_to_cursor, "Zoom to cursor")
                    .on_hover_text("The wheel zooms towards the point under the cursor");
                ui.horizontal(|ui| {
                    ui.label("Default Orbit Type:");
                    for t in [super::camera::OrbitType::Free, super::camera::OrbitType::Constrained] {
                        ui.radio_value(&mut self.orbit_type, t, t.label()).on_hover_text(
                            match t {
                                super::camera::OrbitType::Free => {
                                    "Relative to the screen: a top view tilts, it does not spin about Z"
                                }
                                super::camera::OrbitType::Constrained => {
                                    "Relative to the model: a horizontal drag turns about the model's Z axis"
                                }
                            },
                        );
                    }
                });
                ui.checkbox(
                    &mut self.orbit_at_cursor,
                    "Orbit about the model point under the cursor",
                )
                .on_hover_text(
                    "Orbit pivots on the face under the cursor; over empty space it turns about the centre of the screen",
                );
                ui.checkbox(
                    &mut self.fit_new_geometry,
                    "Zoom out when new geometry leaves the view",
                )
                .on_hover_text(
                    "A sketch made larger (typed sizes, dimensions) or a placed component that no longer fits: the view zooms out to show everything",
                );
                ui.separator();
                ui.label(RichText::new("Language").strong());
                let mut bg = super::i18n::bulgarian();
                if ui
                    .checkbox(&mut bg, "Hints in Bulgarian (подсказки на български)")
                    .changed()
                {
                    super::i18n::set_bulgarian(bg);
                }
                ui.separator();
                ui.label(RichText::new("Sketch").strong());
                let o = &mut self.sketch_opts;
                ui.checkbox(
                    &mut o.auto_project_origin,
                    "Autoproject part origin on sketch create",
                );
                ui.checkbox(
                    &mut o.auto_reference_edges,
                    "Autoproject edges of the face on sketch create",
                );
                ui.checkbox(
                    &mut o.look_at_on_create,
                    "Look at sketch plane on sketch creation and edit",
                );
                ui.checkbox(&mut o.show_grid, "Grid lines");
                ui.checkbox(&mut o.show_axes, "Axes");
                ui.checkbox(&mut o.show_triad, "Coordinate system indicator");
                ui.separator();
                ui.label(RichText::new("Save").strong());
                let mut on = self.thumbnail_source != super::layout::ThumbnailSource::Off;
                if ui
                    .checkbox(&mut on, "Save preview picture")
                    .on_hover_text(
                        "A picture of the active document saved in the file, shown by the Open dialog",
                    )
                    .changed()
                {
                    self.thumbnail_source = if on {
                        super::layout::ThumbnailSource::Iso
                    } else {
                        super::layout::ThumbnailSource::Off
                    };
                }
                ui.add_enabled_ui(on, |ui| {
                    ui.indent("thumbnail_source", |ui| {
                        ui.radio_value(
                            &mut self.thumbnail_source,
                            super::layout::ThumbnailSource::Iso,
                            "Isometric view on save",
                        )
                        .on_hover_text("From the front-right-top corner, zoomed to fit (the same model gives the same file)");
                        ui.radio_value(
                            &mut self.thumbnail_source,
                            super::layout::ThumbnailSource::ActiveView,
                            "Active view on save",
                        )
                        .on_hover_text("The view's direction at save, zoomed to fit");
                        ui.add_enabled(false, egui::Button::new("Import from File..."))
                            .on_disabled_hover_text(
                                "Planned: a picture from a file (docs/spec/0.6-files.md §3.0a)",
                            );
                    });
                });
                ui.label("New documents");
                ui.indent("new_documents", |ui| {
                    ui.radio_value(&mut self.docs.separate, false, "In one file")
                        .on_hover_text("New Part / Assembly / Drawing go into the open design file (Manage › Documents lists them)");
                    ui.radio_value(&mut self.docs.separate, true, "As separate files")
                        .on_hover_text("Each new document is a new file in a tab of its own");
                });
                ui.separator();
                ui.label(RichText::new("Display").strong());
                ui.checkbox(&mut self.show_labels, "Show constraint labels");
                if ui
                    .checkbox(&mut self.thread_texture, "Thread texture")
                    .on_hover_text(
                        "Cosmetic threads (tapped holes, Thread) as a banded texture on their faces",
                    )
                    .changed()
                {
                    self.scene_dirty = true;
                }
                ui.checkbox(&mut self.sketch_toolbar, "Floating toolbar in drawing sketches")
                    .on_hover_text(
                        "A drawing's view sketch (a section line) shows its tools in a bar over the sheet; off: they are in its right-click menu",
                    );
                ui.checkbox(&mut self.internal_names, "Internal names in hover text")
                    .on_hover_text(
                        "Name faces and edges under the cursor by their persistent names (Extrusion1:End), as used in files and journals; off: kind, feature and body",
                    );
                ui.separator();
                ui.label(RichText::new("Prompted Interaction").strong());
                ui.checkbox(&mut self.alias_input, "Show command alias input dialog")
                    .on_hover_text(
                        "A letter typed over the view opens a field next to the cursor with the matching commands; Enter or Space starts one (off: a letter starts its command at once)",
                    );
                ui.separator();
                ui.label(RichText::new("Constraint Settings — Inference").strong());
                let k = &mut self.tools.inference;
                ui.checkbox(&mut k.infer, "Infer constraints")
                    .on_hover_text(
                        "Show the constraint the cursor would create (Ctrl suppresses it)",
                    );
                ui.add_enabled_ui(k.infer, |ui| {
                    ui.checkbox(&mut k.persist, "Persist constraints")
                        .on_hover_text("Create the inferred constraints");
                    ui.checkbox(&mut k.point_alignment, "Point alignment")
                        .on_hover_text(
                            "Align new points with existing ones (dotted lines); with Persist, \
                             a line end gets Horizontal/Vertical to the aligned point",
                        );
                    ui.label(RichText::new("Selection for constraint inference").small());
                    ui.horizontal_wrapped(|ui| {
                        ui.checkbox(&mut k.horizontal, "Horizontal");
                        ui.checkbox(&mut k.vertical, "Vertical");
                        ui.checkbox(&mut k.parallel, "Parallel");
                        ui.checkbox(&mut k.perpendicular, "Perpendicular");
                        ui.checkbox(&mut k.midpoint, "Midpoint");
                        ui.checkbox(&mut k.on_curve, "On Curve");
                    });
                });
                ui.checkbox(&mut k.coincident, "Coincident (snap to and share points)")
                    .on_hover_text("Off: new lines do not join existing points");
            });
        self.show_options = open;
    }
}

/// Default width of the label column in property rows.
const LABEL_W: f32 = 82.0;

fn label_width_id() -> egui::Id {
    egui::Id::new("props_label_width")
}

/// The label column's width now (dragged by the user, see [`plabel`]).
pub(super) fn label_width(ctx: &egui::Context) -> f32 {
    ctx.data(|d| d.get_temp::<f32>(label_width_id()))
        .unwrap_or(LABEL_W)
}

/// Sets the width from the saved layout (until the user drags it).
pub(super) fn seed_label_width(ctx: &egui::Context, w: f32) {
    ctx.data_mut(|d| {
        if d.get_temp::<f32>(label_width_id()).is_none() {
            d.insert_temp(label_width_id(), w.clamp(40.0, 400.0));
        }
    });
}

/// The width when the user changed it (to save it).
pub(super) fn dragged_label_width(ctx: &egui::Context) -> Option<f32> {
    ctx.data(|d| d.get_temp::<f32>(label_width_id()))
}

/// A collapsible section; `body` gets a two-column grid (label, field).
pub(super) fn psection<R>(
    ui: &mut egui::Ui,
    id: &str,
    title: &str,
    body: impl FnOnce(&mut egui::Ui) -> R,
) -> Option<R> {
    ui.add_space(2.0);
    let r = egui::collapsing_header::CollapsingState::load_with_default_open(
        ui.ctx(),
        ui.make_persistent_id(("psection", id)),
        true,
    )
    .show_header(ui, |ui| {
        ui.label(RichText::new(title).strong());
    })
    .body(|ui| {
        egui::Grid::new(("pgrid", id))
            .num_columns(2)
            .min_col_width(label_width(ui.ctx()))
            .spacing([8.0, 6.0])
            .show(ui, body)
            .inner
    });
    ui.separator();
    r.2.map(|x| x.inner)
}

/// A label cell of a property row (left column of a [`psection`] grid).
/// Its right border drags the column wider or narrower in every panel
/// (the width is remembered in the layout).
pub(super) fn plabel(ui: &mut egui::Ui, text: &str) {
    let color = ui.visuals().weak_text_color();
    let w = label_width(ui.ctx());
    let r = ui.allocate_ui_with_layout(
        egui::vec2(w, 18.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_width(w);
            ui.set_max_width(w);
            ui.add(egui::Label::new(RichText::new(text).color(color)).truncate());
        },
    );
    let rect = r.response.rect;
    let handle = egui::Rect::from_min_max(
        egui::pos2(rect.right() + 1.0, rect.top()),
        egui::pos2(rect.right() + 7.0, rect.bottom()),
    );
    // The cell's own (auto) id: many rows share a text, empty ones too.
    let id = r.response.id.with("plabel_split");
    let h = ui.interact(handle, id, egui::Sense::drag());
    if h.hovered() || h.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeColumn);
        ui.painter().vline(
            handle.center().x,
            handle.y_range(),
            ui.visuals().widgets.hovered.fg_stroke,
        );
    }
    if h.dragged() {
        let nw = (w + h.drag_delta().x).clamp(40.0, 400.0);
        ui.ctx().data_mut(|d| d.insert_temp(label_width_id(), nw));
    }
}

/// A group of icon buttons choosing one value (Inventor's Direction and
/// Output button groups). Returns whether the value changed.
fn icon_choice<T: PartialEq + Copy>(
    ui: &mut egui::Ui,
    value: &mut T,
    options: &[(T, Icon, &str)],
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for &(v, icon, tip) in options {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::click());
            let on = *value == v;
            let vis = ui.visuals();
            let fill = if on {
                vis.selection.bg_fill
            } else if resp.hovered() {
                vis.widgets.hovered.weak_bg_fill
            } else {
                vis.widgets.inactive.weak_bg_fill
            };
            ui.painter().rect_filled(rect, 3.0, fill);
            if on {
                ui.painter()
                    .rect_stroke(rect, 3.0, vis.selection.stroke, egui::StrokeKind::Inside);
            }
            super::icons::paint(
                ui.painter(),
                rect.shrink(4.0),
                icon,
                &super::icons::Palette::ENABLED,
            );
            if resp.on_hover_text(tip).clicked() && !on {
                *value = v;
                changed = true;
            }
        }
    });
    changed
}

/// Inventor's yellow warning box.
pub(super) fn warning_box(ui: &mut egui::Ui, text: &str) {
    egui::Frame::new()
        .fill(Color32::from_rgb(255, 244, 200))
        .stroke(egui::Stroke::new(1.0, Color32::from_rgb(220, 170, 40)))
        .corner_radius(3.0)
        .inner_margin(6.0)
        .show(ui, |ui| {
            ui.label(
                RichText::new(format!("Warning: {text}")).color(Color32::from_rgb(110, 70, 0)),
            );
        });
}

/// Display text of a revolve axis.
fn axis_label(axis: &crate::part::AxisRef, sketch: Option<&crate::sketch::Sketch>) -> String {
    match axis {
        crate::part::AxisRef::Origin(k) => {
            format!("{} Axis", ["X", "Y", "Z"].get(*k).copied().unwrap_or("?"))
        }
        crate::part::AxisRef::SketchLine(id) => {
            let centre = sketch.is_some_and(|s| s.style(*id) == crate::sketch::Style::Centerline);
            if centre {
                "Centerline".to_owned()
            } else {
                "Sketch line".to_owned()
            }
        }
        crate::part::AxisRef::Work(id) => format!("Work axis {id}"),
    }
}

/// The part's work axes (id, display name), for the revolve axis rows.
fn work_axes(part: &crate::part::PartDocument) -> Vec<(String, String)> {
    part.features
        .iter()
        .filter_map(|f| match f {
            Feature::Work(w) if matches!(w.def, crate::part::WorkDef::Axis(_)) => {
                Some((w.id.clone(), w.name.clone()))
            }
            _ => None,
        })
        .collect()
}

/// Work axes as revolve axes (one grid row, only when the part has some).
fn work_axis_buttons(ui: &mut egui::Ui, axes: &[(String, String)]) -> Option<crate::part::AxisRef> {
    if axes.is_empty() {
        return None;
    }
    let mut out = None;
    plabel(ui, "Work axis");
    ui.horizontal_wrapped(|ui| {
        for (id, name) in axes {
            if ui
                .small_button(name)
                .on_hover_text("Revolve around this work axis (must lie in the sketch plane)")
                .clicked()
            {
                out = Some(crate::part::AxisRef::Work(id.clone()));
            }
        }
    });
    ui.end_row();
    out
}

/// Origin axes as revolve axes (one grid row); returns the one clicked.
fn origin_axis_buttons(ui: &mut egui::Ui) -> Option<crate::part::AxisRef> {
    let mut out = None;
    plabel(ui, "Origin axis");
    ui.horizontal(|ui| {
        for (k, n) in ["X", "Y", "Z"].iter().enumerate() {
            if ui
                .small_button(*n)
                .on_hover_text(format!(
                    "Revolve around the {n} axis (must lie in the sketch plane)"
                ))
                .clicked()
            {
                out = Some(crate::part::AxisRef::Origin(k));
            }
        }
    });
    ui.end_row();
    out
}

/// What the Extrude / Revolve panels need to know about the command.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct DraftCtx {
    /// The part has no solid yet (Inventor: no Output operation).
    pub base: bool,
    /// The To target is a curved face (Alternate Solution applies).
    pub curved_to: bool,
    /// A primitive's feature (solid only; Sphere / Torus: Full only).
    pub primitive: bool,
}

/// Inventor's Output section: Solid / Surface and the Boolean operation.
fn output_options(
    ui: &mut egui::Ui,
    id: &str,
    op: &mut crate::part::Operation,
    output: &mut crate::part::Output,
    solids: &mut Vec<String>,
    bodies: &[(String, String)],
    ctx: DraftCtx,
) {
    use crate::part::{Operation, Output};
    psection(ui, &format!("{id}_output"), "Output", |ui| {
        if ctx.primitive {
            // Inventor: primitives make solids only.
            *output = Output::Solid;
        } else {
            plabel(ui, "Output");
            icon_choice(
                ui,
                output,
                &[
                    (Output::Solid, Icon::OutSolid, "Solid"),
                    (
                        Output::Surface,
                        Icon::OutSurface,
                        "Surface: an open body, no caps",
                    ),
                ],
            );
            ui.end_row();
        }
        // Inventor: the base feature (the first solid) has no operation.
        if ctx.base {
            plabel(ui, "");
            ui.label(
                RichText::new("The first solid of the part (no operation)")
                    .small()
                    .weak(),
            );
            ui.end_row();
            return;
        }
        plabel(ui, "Operation");
        ui.add_enabled_ui(*output == Output::Solid, |ui| {
            icon_choice(
                ui,
                op,
                &[
                    (Operation::Join, Icon::OpJoin, "Join: add to the body"),
                    (Operation::Cut, Icon::OpCut, "Cut: remove from the body"),
                    (
                        Operation::Intersect,
                        Icon::OpIntersect,
                        "Intersect: keep only the common volume",
                    ),
                    (
                        Operation::NewSolid,
                        Icon::OpNewSolid,
                        "New Solid: a separate body",
                    ),
                ],
            );
        });
        ui.end_row();
        plabel(ui, "");
        let text = if *output == Output::Surface {
            "Surface".to_owned()
        } else {
            op.label().to_owned()
        };
        ui.label(RichText::new(text).small().weak());
        ui.end_row();
        // Inventor's Solids: with several bodies, the ones to operate on.
        if bodies.len() > 1 && *output == Output::Solid && *op != Operation::NewSolid {
            plabel(ui, "Solids");
            ui.vertical(|ui| {
                for (name, label) in bodies {
                    let mut on = solids.contains(name);
                    if ui
                        .checkbox(&mut on, label.as_str())
                        .on_hover_text(format!("Body created by {name}"))
                        .changed()
                    {
                        if on {
                            solids.push(name.clone());
                        } else {
                            solids.retain(|n| n != name);
                        }
                    }
                }
                let hint = if !solids.is_empty() {
                    String::new()
                } else if *op == Operation::Join {
                    format!("None chosen: {}", bodies[0].1)
                } else {
                    "None chosen: all bodies".to_owned()
                };
                if !hint.is_empty() {
                    ui.label(RichText::new(hint).small().weak());
                }
            });
            ui.end_row();
        }
    });
}

/// Direction buttons (Default / Flipped / Symmetric / Asymmetric).
fn direction_choice(ui: &mut egui::Ui, dir: &mut crate::part::ExtentDirection) {
    use crate::part::ExtentDirection as D;
    plabel(ui, "Direction");
    icon_choice(
        ui,
        dir,
        &[
            (D::Default, Icon::DirDefault, "Default"),
            (D::Flipped, Icon::DirFlipped, "Flipped"),
            (D::Symmetric, Icon::DirSymmetric, "Symmetric"),
            (D::Asymmetric, Icon::DirAsymmetric, "Asymmetric: two values"),
        ],
    );
    ui.end_row();
}

/// Parameter fields of one dialog: the part's parameters (to evaluate
/// and list) and the texts committed this frame.
pub(super) struct Fields<'a> {
    /// The part's parameter table.
    pub table: &'a [crate::part::params::ParamRow],
    /// Id prefix (draft / feature k).
    pub key: String,
    /// (field, text) committed this frame.
    pub commits: Vec<(&'static str, String)>,
    /// Bodies the feature may operate on: (name, display name).
    pub bodies: Vec<(String, String)>,
}

/// One direction of a rectangular pattern in the panel: Flip, Count,
/// Incremental / Fitted, Spacing or Distance, Midplane.
fn pattern_direction_rows(
    ui: &mut egui::Ui,
    dir: &mut crate::part::PatternDirection,
    fx: &mut Fields,
    f: &Feature,
    count: &'static str,
    spacing: &'static str,
) {
    plabel(ui, "Flip");
    ui.checkbox(&mut dir.flip, "")
        .on_hover_text("Reverse the direction");
    ui.end_row();
    field_row(ui, fx, "Count", f, count);
    plabel(ui, "Distribution");
    ui.horizontal(|ui| {
        ui.selectable_value(&mut dir.fitted, false, "Incremental")
            .on_hover_text("The spacing between instances");
        ui.selectable_value(&mut dir.fitted, true, "Fitted")
            .on_hover_text("The distance from the first instance to the last");
    });
    ui.end_row();
    let label = if dir.fitted { "Distance" } else { "Spacing" };
    field_row(ui, fx, label, f, spacing);
    plabel(ui, "Midplane");
    ui.checkbox(&mut dir.midplane, "")
        .on_hover_text("Instances on both sides of the original");
    ui.end_row();
}

/// A numeric feature field that takes expressions (one grid row).
pub(super) fn field_row(
    ui: &mut egui::Ui,
    fx: &mut Fields,
    label: &str,
    f: &Feature,
    field: &'static str,
) {
    use crate::part::params::{feature_field_param, feature_field_unit, feature_value};
    plabel(ui, label);
    let p = feature_field_param(f, field);
    let name = p
        .as_ref()
        .map(|p| p.name.as_str())
        .filter(|n| !n.is_empty());
    let spec = super::valuefield::FieldSpec {
        id: egui::Id::new(("field", fx.key.as_str(), field)),
        value: feature_value(f, field).unwrap_or_default(),
        unit: feature_field_unit(f, field).unwrap_or_default(),
        name,
        expr: p.as_ref().and_then(|p| p.expr.as_deref()),
        table: fx.table,
    };
    if let Some(t) = super::valuefield::value_field(ui, &spec) {
        fx.commits.push((field, t));
    }
    ui.end_row();
}

/// The Revolve options (Inventor's Behavior / Output sections). Returns
/// whether an option changed.
fn revolve_options(
    ui: &mut egui::Ui,
    def: &mut crate::part::RevolveFeature,
    fx: &mut Fields,
    picking: Option<super::TargetSlot>,
    pick: &mut Option<super::TargetSlot>,
    ctx: DraftCtx,
) -> bool {
    use crate::part::{ExtentDirection, RevolveExtent};
    let before = def.clone();
    psection(ui, "revolve_behavior", "Behavior", |ui| {
        plabel(ui, "Extents");
        if ctx.primitive {
            // Inventor: Sphere and Torus are full revolutions only.
            def.extent = RevolveExtent::Full;
            ui.label("Full");
        } else {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut def.extent, RevolveExtent::Full, "Full")
                    .on_hover_text("360 deg");
                ui.selectable_value(&mut def.extent, RevolveExtent::Angle, "Angle");
                if ui
                    .selectable_label(def.extent == RevolveExtent::To, "To")
                    .on_hover_text(
                        "Up to a planar face or a plane through the axis, or a point (its half-plane)",
                    )
                    .clicked()
                {
                    def.extent = RevolveExtent::To;
                    if def.to.is_none() {
                        *pick = Some(super::TargetSlot::To);
                    }
                }
            });
        }
        ui.end_row();
        if def.extent == RevolveExtent::To {
            plabel(ui, "To");
            ui.horizontal(|ui| {
                let active = picking == Some(super::TargetSlot::To);
                let text = match &def.to {
                    _ if active => "Picking...".to_owned(),
                    Some(t) => t.name(),
                    None => "(none)".to_owned(),
                };
                if ui
                    .selectable_label(active, text)
                    .on_hover_text(
                        "Click, then pick a face, a plane or a point in the graphics window",
                    )
                    .clicked()
                {
                    *pick = Some(super::TargetSlot::To);
                }
            });
            ui.end_row();
            plabel(ui, "Direction");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut def.direction, ExtentDirection::Default, "Default");
                ui.selectable_value(&mut def.direction, ExtentDirection::Flipped, "Flipped");
            });
            ui.end_row();
            plabel(ui, "");
            ui.checkbox(&mut def.minimum, "Minimum Solution")
                .on_hover_text("A plane through the axis has two halves: stop at the nearer one (off: the farther one)");
            ui.end_row();
        }
        if def.extent == RevolveExtent::Angle {
            direction_choice(ui, &mut def.direction);
            let f = Feature::Revolve(def.clone());
            field_row(ui, fx, "Angle A", &f, "angle");
            if def.direction == ExtentDirection::Asymmetric {
                field_row(ui, fx, "Angle B", &f, "angle_b");
            }
        }
    });
    output_options(
        ui,
        "revolve",
        &mut def.operation,
        &mut def.output,
        &mut def.solids,
        &fx.bodies,
        ctx,
    );
    // Open curves: only a surface takes them (Inventor).
    if def.output == crate::part::Output::Surface {
        egui::Grid::new("revolve_open").num_columns(2).show(ui, |ui| {
            plabel(ui, "Open curves");
            ui.horizontal(|ui| {
                ui.label(format!("{}", def.open.len())).on_hover_text(
                    "Click curves of the sketch that close no area to revolve them as a surface (a second click removes one)",
                );
                if !def.open.is_empty() && ui.small_button("x").on_hover_text("Clear").clicked() {
                    def.open.clear();
                }
            });
            ui.end_row();
        });
    } else if !def.open.is_empty() {
        def.open.clear();
    }
    *def != before
}

/// The Extrude options (Inventor's Behavior / Output / Advanced sections).
/// `picking` is the target slot being picked; a click on "Pick" sets
/// `pick`. Returns whether an option changed.
fn extrude_options(
    ui: &mut egui::Ui,
    def: &mut crate::part::ExtrudeFeature,
    picking: Option<super::TargetSlot>,
    pick: &mut Option<super::TargetSlot>,
    fx: &mut Fields,
    ctx: DraftCtx,
) -> bool {
    use super::TargetSlot;
    use crate::part::{Extent, ExtentDirection};
    let before = def.clone();
    // A target field: name, Pick, clear (one grid row). Returns "clear".
    let target_row = |ui: &mut egui::Ui,
                      label: &str,
                      current: Option<&crate::part::TargetRef>,
                      slot: TargetSlot,
                      pick: &mut Option<TargetSlot>|
     -> bool {
        let mut clear = false;
        plabel(ui, label);
        ui.horizontal(|ui| {
            let active = picking == Some(slot);
            let text = match current {
                _ if active => "Picking...".to_owned(),
                Some(t) => t.name().to_owned(),
                None => "Sketch plane".to_owned(),
            };
            if ui
                .selectable_label(active, text)
                .on_hover_text("Click, then pick a face or a vertex in the graphics window")
                .clicked()
            {
                *pick = Some(slot);
            }
            if current.is_some() && ui.small_button("x").on_hover_text("Clear").clicked() {
                clear = true;
            }
        });
        ui.end_row();
        clear
    };

    psection(ui, "extrude_behavior", "Behavior", |ui| {
        if target_row(ui, "From", def.from.as_ref(), TargetSlot::From, pick) {
            def.from = None;
        }
        direction_choice(ui, &mut def.direction);
        let kind = match def.extent {
            Extent::Distance => 0,
            Extent::To(_) => 1,
            Extent::ToNext => 2,
            Extent::ThroughAll => 3,
        };
        let names = ["Distance", "To", "To Next", "Through All"];
        let mut chosen = kind;
        plabel(ui, "Extents");
        egui::ComboBox::from_id_salt("extrude_extents")
            .width(110.0)
            .selected_text(names[kind])
            .show_ui(ui, |ui| {
                for (i, n) in names.iter().enumerate() {
                    ui.selectable_value(&mut chosen, i, *n);
                }
            });
        ui.end_row();
        if chosen != kind {
            def.extent = match chosen {
                0 => Extent::Distance,
                1 => {
                    *pick = Some(TargetSlot::To);
                    Extent::Distance // until a target is picked
                }
                2 => Extent::ToNext,
                _ => Extent::ThroughAll,
            };
        }
        match &def.extent {
            Extent::Distance => {
                let f = Feature::Extrude(def.clone());
                field_row(ui, fx, "Distance A", &f, "distance");
                if def.direction == ExtentDirection::Asymmetric {
                    field_row(ui, fx, "Distance B", &f, "distance_b");
                }
                if picking == Some(TargetSlot::To) {
                    plabel(ui, "To");
                    ui.label(RichText::new("Pick the face or vertex...").small().weak());
                    ui.end_row();
                }
            }
            Extent::To(t) => {
                let t = t.clone();
                if target_row(ui, "To", Some(&t), TargetSlot::To, pick) {
                    def.extent = Extent::Distance;
                }
                if def.from.is_some() {
                    plabel(ui, "");
                    ui.label(RichText::new("From + To: Between").small().weak());
                    ui.end_row();
                }
            }
            Extent::ToNext => {
                // Inventor's Terminator: the body whose next face ends it.
                plabel(ui, "Terminator");
                let current = def
                    .terminator
                    .as_ref()
                    .and_then(|n| fx.bodies.iter().find(|(b, _)| b == n))
                    .map_or("Any body".to_owned(), |(_, l)| l.clone());
                egui::ComboBox::from_id_salt(("extrude_terminator", fx.key.as_str()))
                    .width(110.0)
                    .selected_text(current)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut def.terminator, None, "Any body");
                        for (name, label) in &fx.bodies {
                            ui.selectable_value(&mut def.terminator, Some(name.clone()), label);
                        }
                    })
                    .response
                    .on_hover_text(
                        "The next face (covering the profile) of this body ends the extrusion",
                    );
                ui.end_row();
            }
            Extent::ThroughAll => {
                plabel(ui, "");
                ui.label(RichText::new("To the far side of the body").small().weak());
                ui.end_row();
            }
        }
    });

    output_options(
        ui,
        "extrude",
        &mut def.operation,
        &mut def.output,
        &mut def.solids,
        &fx.bodies,
        ctx,
    );

    psection(ui, "extrude_advanced", "Advanced Properties", |ui| {
        let f = Feature::Extrude(def.clone());
        field_row(ui, fx, "Taper A", &f, "taper");
        if def.direction == ExtentDirection::Asymmetric {
            field_row(ui, fx, "Taper B", &f, "taper_b");
        }
        if def.from.is_some() || matches!(def.extent, Extent::To(_)) {
            plabel(ui, "");
            ui.checkbox(&mut def.extend_face, "Extend face to end feature")
                .on_hover_text("A From / To face that does not cover the profile is extended (turns on by itself when needed)");
            ui.end_row();
        }
        if ctx.curved_to {
            plabel(ui, "");
            ui.checkbox(&mut def.alternate, "Alternate Solution")
                .on_hover_text("Stop at the near side of the curved To face (off: the far side)");
            ui.end_row();
        }
        let n_open = def.open.as_ref().map_or(0, |o| o.curves.len());
        let surface = def.output == crate::part::Output::Surface;
        plabel(ui, "");
        match def.open.as_mut() {
            // A surface of open curves: no side to keep, no Match Shape.
            Some(_) if surface => {
                ui.label(RichText::new(format!("Open curves: {n_open}")).small());
            }
            Some(op) => {
                ui.vertical(|ui| {
                    ui.checkbox(&mut op.match_shape, "Match Shape")
                        .on_hover_text("On: the fill stops at the body's walls; off: the closed side goes the full extent");
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("Open profile: {n_open} curve(s)")).small());
                        if ui.small_button("Flip side").on_hover_text("Keep the other side of the open curves").clicked() {
                            op.flip = !op.flip;
                        }
                    });
                });
            }
            None => {
                ui.add_enabled(false, egui::Checkbox::new(&mut false, "Match Shape"))
                    .on_disabled_hover_text(
                        "Click open curves of the sketch (not for the first feature)",
                    );
            }
        }
        ui.end_row();
        plabel(ui, "");
        ui.add_enabled(false, egui::Checkbox::new(&mut false, "Infer iMates"))
            .on_disabled_hover_text("Assemblies (FEATURES 3.x)");
        ui.end_row();
    });
    *def != before
}

impl CadCraftApp {
    /// Manage → **Parameters** (fx): every model and user parameter with
    /// its equation, value, tolerance and comment (Inventor's table).
    /// The Suppress if window: a true / false equation for a feature (empty
    /// removes it).
    pub(super) fn suppress_if_window(&mut self, ctx: &egui::Context) {
        let Some((k, mut cond, mut error)) = self.suppress_if_edit.clone() else {
            return;
        };
        let name = self
            .part()
            .features
            .get(k)
            .map(|f| f.name().to_owned())
            .unwrap_or_default();
        let table = self.part().parameter_table();
        let names = super::if_builder::choices(&table, None);
        let mut apply = false;
        let mut close = false;
        let mut clear = false;
        egui::Window::new(format!("Suppress if: {name}"))
            .default_pos(ctx.content_rect().center() - egui::vec2(200.0, 120.0))
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label("The feature (and what depends on it) is suppressed while this holds:");
                cond.ui(ui, &names, egui::Id::new(("suppress_if", k)));
                let text = cond.text(&names);
                if text.is_empty() {
                    ui.label(
                        RichText::new("No condition: the feature is never suppressed by it").weak(),
                    );
                } else if super::if_builder::unfinished(&cond) {
                    ui.label(RichText::new("Choose a parameter, a comparison and a value").weak());
                } else {
                    ui.label(RichText::new(&text).monospace());
                    let state = self.part().check_condition(&text);
                    super::if_builder::state_label(
                        ui,
                        &state,
                        "the feature is suppressed",
                        "the feature is built",
                    );
                }
                if let Some(e) = &error {
                    ui.label(RichText::new(e).color(WARNING));
                }
                ui.horizontal(|ui| {
                    apply |= ui.button("OK").clicked();
                    close |= ui.button("Cancel").clicked();
                    clear |= ui
                        .button("Remove")
                        .on_hover_text("No condition (the feature is built)")
                        .clicked();
                });
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    apply = true;
                }
            });
        if clear {
            cond = super::if_builder::CondBuilder::from_text("");
            apply = true;
        }
        if apply {
            self.checkpoint("Suppress if");
            let t = cond.text(&names);
            match self
                .part_mut()
                .set_suppress_if(k, (!t.is_empty()).then_some(t.as_str()))
            {
                Ok(()) => {
                    self.suppress_if_edit = None;
                    self.file_status = None;
                    self.scene_dirty = true;
                }
                Err(e) => {
                    self.history.cancel();
                    error = Some(e.to_string());
                    self.suppress_if_edit = Some((k, cond, error));
                }
            }
        } else if close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.suppress_if_edit = None;
        } else {
            self.suppress_if_edit = Some((k, cond, error));
        }
    }

    pub(super) fn parameters_window(&mut self, ctx: &egui::Context) {
        use crate::part::expr::Unit;
        use crate::part::params::ParamOwner;
        if !self.show_params {
            return;
        }
        // The physical properties of the body as it is now (read-only rows).
        let key = self.body_key();
        if key != self.param_phys_key {
            self.param_phys_key = key;
            self.part_mut().update_physical();
        }
        let mut open = true;
        let rows = self.part().parameter_table();
        let errors = self.part().param_errors.clone();
        // Edits are collected and applied after drawing.
        enum Edit {
            Rename(String, String),
            Expr(String, String),
            Eval(usize, crate::sketch::ConstraintId, crate::sketch::EvalSize),
            Comment(ParamOwner, String),
            UnitSpec(usize, String),
            Add,
            Delete(String),
            Link(String, String),
            Purge,
            Key(String, bool),
            Precision(String, Option<u8>),
            Values(usize, Vec<String>),
        }
        use super::valuefield::ParamDrag;
        let mut edits: Vec<Edit> = Vec::new();
        // The row whose "if" button was pressed this frame.
        let mut if_open: Option<String> = None;
        // Filter: names some equation uses (Used / Unused).
        let referenced: std::collections::HashSet<String> = rows
            .iter()
            .filter_map(|r| r.expr.as_deref())
            .chain(self.part().suppress_if.values().map(String::as_str))
            .flat_map(crate::part::expr::names_in)
            .collect();
        let mut filter = self.param_filter;
        let meta = self.part().param_meta.clone();
        let is_key = |n: &str| meta.get(n).is_some_and(|m| m.key);
        let search = self.param_search.to_lowercase();
        let shown = |r: &crate::part::params::ParamRow| {
            let kind = match filter {
                1 => matches!(r.owner, ParamOwner::User(_)),
                2 => !matches!(r.owner, ParamOwner::User(_)),
                3 => referenced.contains(&r.name),
                4 => !referenced.contains(&r.name),
                5 => is_key(&r.name),
                _ => true,
            };
            let found = search.is_empty()
                || r.name.to_lowercase().contains(&search)
                || r.comment.to_lowercase().contains(&search)
                || r.consumed_by.to_lowercase().contains(&search)
                || r.equation().to_lowercase().contains(&search);
            kind && found
        };
        // Used by: the parameters whose equations name a row, and the
        // Suppress if conditions.
        let mut used_by: std::collections::HashMap<String, Vec<String>> = Default::default();
        for r in &rows {
            if let Some(e) = &r.expr {
                for n in crate::part::expr::names_in(e) {
                    used_by.entry(n).or_default().push(r.name.clone());
                }
            }
        }
        for (id, c) in &self.part().suppress_if {
            for n in crate::part::expr::names_in(c) {
                used_by
                    .entry(n)
                    .or_default()
                    .push(format!("Suppress if ({id})"));
            }
        }
        // Suggestions while typing an equation: parameters, functions.
        let mut suggest: Vec<String> = rows.iter().map(|r| r.name.clone()).collect();
        suggest.extend(crate::part::expr::FUNCTIONS.iter().map(|f| format!("{f}(")));
        suggest.extend(["PI".to_owned(), "E".to_owned()]);
        let user_values: Vec<Vec<String>> = self
            .part()
            .user_params
            .iter()
            .map(|u| u.values.clone())
            .collect();
        let rows: Vec<_> = rows.iter().filter(|r| shown(r)).cloned().collect();
        let mut cols = self.param_cols;
        let mut font = self.param_font;
        let mut search_text = self.param_search.clone();
        let mut values_edit = self.param_values_edit.clone();
        let mut csv: Option<bool> = None;
        let buf = &mut self.param_buf;
        // The window keeps the size the user gave it (`layout.place`); the
        // table scrolls inside it both ways.
        self.layout.place("parameters", ctx.content_rect(), true, egui::Window::new("Parameters")
            .open(&mut open)
            .default_size(egui::vec2(900.0, 480.0))
            .min_size(egui::vec2(360.0, 200.0))
            .resizable(true))
            .show(ctx, |ui| {
                // The table's own text size (A- / A+).
                for f in ui.style_mut().text_styles.values_mut() {
                    f.size *= font;
                }
                ui.horizontal(|ui| {
                    ui.label("Search");
                    ui.add(egui::TextEdit::singleline(&mut search_text).desired_width(160.0).hint_text("name, equation, comment"));
                    if !search_text.is_empty() && ui.small_button("x").clicked() {
                        search_text.clear();
                    }
                    ui.separator();
                    if ui.small_button("A-").on_hover_text("Smaller text").clicked() {
                        font = (font - 0.1).max(0.7);
                    }
                    if ui.small_button("A+").on_hover_text("Larger text").clicked() {
                        font = (font + 0.1).min(2.5);
                    }
                });
                ui.separator();
                // A text cell that applies on Enter / focus loss.
                let mut cell = |ui: &mut egui::Ui, key: String, current: &str, width: f32, bad: bool| -> (Option<String>, egui::Response) {
                    let text = buf.entry(key.clone()).or_insert_with(|| current.to_owned());
                    let mut te = egui::TextEdit::singleline(text).desired_width(width * font);
                    if bad {
                        te = te.text_color(Color32::from_rgb(230, 70, 60));
                    }
                    let r = ui.add(te);
                    if !r.has_focus() && !r.lost_focus() && *text != current {
                        *text = current.to_owned(); // changed elsewhere
                    }
                    if key.starts_with("e:") && r.has_focus() {
                        super::if_builder::draw_hint(ui, r.rect.translate(egui::vec2(0.0, 2.0)), text, &rows);
                    }
                    // An equation cell: the names / functions that start with
                    // the word being typed (a click puts it in).
                    if key.starts_with("e:") && r.has_focus() {
                        let word_at = text
                            .char_indices()
                            .rev()
                            .find(|(_, c)| !(c.is_alphanumeric() || *c == '_'))
                            .map_or(0, |(i, c)| i + c.len_utf8());
                        let word = text[word_at..].to_lowercase();
                        let list: Vec<&String> = if word.is_empty() {
                            Vec::new()
                        } else {
                            suggest
                                .iter()
                                .filter(|s| s.to_lowercase().starts_with(&word) && s.to_lowercase() != word)
                                .take(8)
                                .collect()
                        };
                        if !list.is_empty() {
                            let mut pick = None;
                            egui::Area::new(egui::Id::new(("param_suggest", key.as_str())))
                                .fixed_pos(r.rect.left_bottom())
                                .order(egui::Order::Foreground)
                                .show(ui.ctx(), |ui| {
                                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                                        for s in &list {
                                            if ui.selectable_label(false, s.as_str()).clicked() {
                                                pick = Some((*s).clone());
                                            }
                                        }
                                    });
                                });
                            if let Some(s) = pick {
                                text.truncate(word_at);
                                text.push_str(&s);
                                r.request_focus();
                            }
                        }
                    }
                    if r.lost_focus() {
                        let t = text.clone();
                        buf.remove(&key);
                        return ((t != current).then_some(t), r);
                    }
                    (None, r)
                };
                // Pick whip: a parameter dropped on a row's name or equation
                // links that row to it.
                let drop_on = |ui: &egui::Ui, resp: &egui::Response, target: &str| -> Option<String> {
                    if resp.dnd_hover_payload::<ParamDrag>().is_some_and(|p| p.0 != target) {
                        ui.painter().rect_stroke(
                            resp.rect.expand(2.0),
                            2.0,
                            egui::Stroke::new(2.0, Color32::from_rgb(230, 120, 20)),
                            egui::StrokeKind::Outside,
                        );
                    }
                    resp.dnd_release_payload::<ParamDrag>()
                        .filter(|p| p.0 != target)
                        .map(|p| p.0.clone())
                };
                let table_h = (ui.available_height() - 64.0).max(80.0);
                egui::ScrollArea::both()
                    .id_salt("params_scroll")
                    .auto_shrink([false, false])
                    .max_height(table_h)
                    .show(ui, |ui| {
                    egui::Grid::new("params")
                        .striped(true)
                        .num_columns(10)
                        .spacing([10.0, 4.0])
                        .show(ui, |ui| {
                            for (k, h) in [
                                "Parameter Name",
                                "Consumed by",
                                "Used by",
                                "Unit",
                                "Equation",
                                "Nominal Value",
                                "Tolerance",
                                "Model Value",
                                "Comment",
                                "",
                            ]
                            .iter()
                            .enumerate()
                            {
                                // Name, Equation and Comment widen by dragging
                                // the bar after their title.
                                let col = match k {
                                    0 => Some(0),
                                    4 => Some(1),
                                    8 => Some(2),
                                    _ => None,
                                };
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(*h).strong());
                                    if let Some(c) = col {
                                        let bar = ui
                                            .add(egui::Label::new(RichText::new("|").weak()).sense(egui::Sense::drag()))
                                            .on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
                                            .on_hover_text("Drag to widen the column");
                                        if bar.dragged() {
                                            cols[c] = (cols[c] + bar.drag_delta().x).clamp(40.0, 600.0);
                                        }
                                    }
                                });
                            }
                            ui.end_row();
                            let mut section = "";
                            for r in &rows {
                                let this = if matches!(r.owner, ParamOwner::User(_)) {
                                    "User Parameters"
                                } else {
                                    "Model Parameters"
                                };
                                if this != section {
                                    section = this;
                                    ui.label(RichText::new(this).strong().color(Color32::from_rgb(120, 170, 240)));
                                    ui.end_row();
                                }
                                let (renamed, name_resp) = ui
                                    .horizontal(|ui| {
                                        let h = ui
                                            .add(
                                                egui::Label::new(
                                                    RichText::new("@")
                                                        .strong()
                                                        .color(Color32::from_rgb(230, 120, 20)),
                                                )
                                                .sense(egui::Sense::drag()),
                                            )
                                            .on_hover_text(
                                                "Pick whip: drag onto another parameter or onto any \
                                                 value field to make it follow this one",
                                            );
                                        h.dnd_set_drag_payload(ParamDrag(r.name.clone()));
                                        if h.drag_started() {
                                            super::valuefield::whip_start(ui.ctx(), h.rect.center());
                                        }
                                        if matches!(r.owner, ParamOwner::Physical(_)) {
                                            // Measured: read only.
                                            let l = ui.label(RichText::new(&r.name).italics());
                                            (None, l)
                                        } else {
                                            if is_key(&r.name) {
                                                ui.label(RichText::new("★").color(Color32::from_rgb(230, 170, 20)));
                                            }
                                            cell(ui, format!("n:{}", r.name), &r.name, cols[0], false)
                                        }
                                    })
                                    .inner;
                                if let Some(n) = renamed {
                                    edits.push(Edit::Rename(r.name.clone(), n));
                                }
                                if let Some(src) = drop_on(ui, &name_resp, &r.name) {
                                    edits.push(Edit::Link(r.name.clone(), src));
                                }
                                // The row's menu: Key, precision, Multi-Value.
                                name_resp.context_menu(|ui| {
                                    let key_on = is_key(&r.name);
                                    if ui.button(if key_on { "Unmark Key" } else { "Mark as Key" }).clicked() {
                                        edits.push(Edit::Key(r.name.clone(), !key_on));
                                        ui.close();
                                    }
                                    ui.menu_button("Precision", |ui| {
                                        if ui.button("Default").clicked() {
                                            edits.push(Edit::Precision(r.name.clone(), None));
                                            ui.close();
                                        }
                                        for d in 0..=6u8 {
                                            if ui.button(format!("{d} decimals")).clicked() {
                                                edits.push(Edit::Precision(r.name.clone(), Some(d)));
                                                ui.close();
                                            }
                                        }
                                    });
                                    if let ParamOwner::User(i) = r.owner {
                                        let list = user_values.get(i).cloned().unwrap_or_default();
                                        if ui.button("Edit Multi-Value List...").clicked() {
                                            let text = if list.is_empty() { r.equation() } else { list.join("; ") };
                                            values_edit = Some((i, text));
                                            ui.close();
                                        }
                                        if !list.is_empty() && ui.button("Make Single Value").clicked() {
                                            edits.push(Edit::Values(i, Vec::new()));
                                            ui.close();
                                        }
                                    }
                                });
                                ui.label(&r.consumed_by);
                                let users = used_by.get(&r.name).map(|v| v.join(", ")).unwrap_or_default();
                                ui.label(RichText::new(&users).weak()).on_hover_text(if users.is_empty() { "No equation uses it".to_owned() } else { format!("Used in: {users}") });
                                match r.owner {
                                    ParamOwner::User(i) => {
                                        // Unit Specification (`N/m`, `kg`, `MPa`)
                                        // typed, or picked by kind (Inventor's
                                        // Unit Type).
                                        ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing.x = 1.0;
                                            let (typed, _) = cell(ui, format!("u:{}", r.name), &r.unit_label(), 56.0, false);
                                            if let Some(t) = typed {
                                                edits.push(Edit::UnitSpec(i, t));
                                            }
                                            ui.menu_button("...", |ui| {
                                                for (kind, list) in crate::part::units::CATEGORIES {
                                                    ui.menu_button(*kind, |ui| {
                                                        for u in *list {
                                                            if ui.button(*u).clicked() {
                                                                edits.push(Edit::UnitSpec(i, (*u).to_owned()));
                                                                ui.close();
                                                            }
                                                        }
                                                    });
                                                }
                                            })
                                            .response
                                            .on_hover_text("Unit Type: pick a unit by kind, or type one (N/m, kg/m^3, km/h)");
                                        });
                                    }
                                    _ => {
                                        ui.label(r.unit_label());
                                    }
                                }
                                let err = errors.get(&r.name);
                                let (eq, eq_resp) = if matches!(r.owner, ParamOwner::Physical(_)) {
                                    let l = ui
                                        .label(RichText::new("(measured)").weak())
                                        .on_hover_text("Measured on the body after each rebuild; use it in user parameters, not in sizes");
                                    (None, l)
                                } else {
                                    match r.owner {
                                        // Multi-Value: pick one of the list.
                                        ParamOwner::User(i) if user_values.get(i).is_some_and(|v| !v.is_empty()) => {
                                            let list = user_values[i].clone();
                                            let cur = r.equation();
                                            let resp = egui::ComboBox::from_id_salt(("multi", i))
                                                .width(cols[1] * font)
                                                .selected_text(&cur)
                                                .show_ui(ui, |ui| {
                                                    for v in &list {
                                                        if ui.selectable_label(*v == cur, v.as_str()).clicked() && *v != cur {
                                                            edits.push(Edit::Expr(r.name.clone(), v.clone()));
                                                        }
                                                    }
                                                })
                                                .response;
                                            (None, resp)
                                        }
                                        _ => ui
                                            .horizontal(|ui| {
                                                ui.spacing_mut().item_spacing.x = 2.0;
                                                let c = cell(ui, format!("e:{}", r.name), &r.equation(), cols[1], err.is_some());
                                                if ui
                                                    .small_button("if")
                                                    .on_hover_text("If...: this value depends on a condition (built from lists)")
                                                    .clicked()
                                                {
                                                    if_open = Some(r.name.clone());
                                                }
                                                c
                                            })
                                            .inner,
                                    }
                                };
                                if let Some(src) = drop_on(ui, &eq_resp, &r.name) {
                                    edits.push(Edit::Link(r.name.clone(), src));
                                }
                                if let Some(e) = err {
                                    ui.ctx().debug_text(e.clone());
                                }
                                if let Some(t) = eq {
                                    edits.push(Edit::Expr(r.name.clone(), t));
                                }
                                let shown_value = |v: crate::kernel::Real| match meta.get(&r.name).and_then(|m| m.precision) {
                                    Some(d) => {
                                        let x = r.def().to_display(v);
                                        let l = r.unit_label();
                                        if l.is_empty() || l == "ul" { format!("{x:.*}", d as usize) } else { format!("{x:.*} {l}", d as usize) }
                                    }
                                    None => crate::part::params::format_in(v, r.def(), &r.unit_label()),
                                };
                                ui.label(shown_value(r.value));
                                match (&r.owner, r.tol) {
                                    (ParamOwner::Dimension { sketch, constraint }, Some(t)) => {
                                        let mut ev = t.eval;
                                        egui::ComboBox::from_id_salt(("eval", &r.name))
                                            .width(70.0)
                                            .selected_text(ev.label())
                                            .show_ui(ui, |ui| {
                                                for x in crate::sketch::EvalSize::ALL {
                                                    ui.selectable_value(&mut ev, x, x.label());
                                                }
                                            });
                                        if ev != t.eval {
                                            edits.push(Edit::Eval(*sketch, *constraint, ev));
                                        }
                                        ui.label(shown_value(r.value + t.offset()));
                                    }
                                    _ => {
                                        ui.label("");
                                        ui.label(shown_value(r.value));
                                    }
                                }
                                match r.owner {
                                    ParamOwner::Feature { .. } => {
                                        ui.label("");
                                    }
                                    _ => {
                                        if let (Some(c), _) = cell(ui, format!("c:{}", r.name), &r.comment, cols[2], false) {
                                            edits.push(Edit::Comment(r.owner.clone(), c));
                                        }
                                    }
                                }
                                if matches!(r.owner, ParamOwner::User(_)) {
                                    if ui.small_button("x").on_hover_text("Delete (only if unused)").clicked() {
                                        edits.push(Edit::Delete(r.name.clone()));
                                    }
                                } else {
                                    ui.label("");
                                }
                                ui.end_row();
                                if let Some(e) = err {
                                    ui.label("");
                                    ui.colored_label(Color32::from_rgb(230, 70, 60), format!("{}: {e}", r.name));
                                    ui.end_row();
                                }
                            }
                        });
                });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Add Numeric").clicked() {
                        edits.push(Edit::Add);
                    }
                    if ui
                        .button("Purge Unused")
                        .on_hover_text("Delete the user parameters nothing uses")
                        .clicked()
                    {
                        edits.push(Edit::Purge);
                    }
                    egui::ComboBox::from_id_salt("param_filter")
                        .width(90.0)
                        .selected_text(["All", "User", "Model", "Used", "Unused", "Key"][filter.min(5) as usize])
                        .show_ui(ui, |ui| {
                            for (k, t) in ["All", "User", "Model", "Used", "Unused", "Key"].iter().enumerate() {
                                ui.selectable_value(&mut filter, k as u8, *t);
                            }
                        })
                        .response
                        .on_hover_text("Filter");
                    ui.separator();
                    if ui.button("Export CSV...").on_hover_text("Name, Unit, Equation, Value, Comment (opens in Excel)").clicked() {
                        csv = Some(true);
                    }
                    if ui.button("Import CSV...").on_hover_text("Known names take the equation and comment; new names become user parameters").clicked() {
                        csv = Some(false);
                    }
                });
                ui.label(
                    RichText::new("Drag @ onto a parameter or a value field to link them; right-click a name for Key, Precision, Multi-Value. Equations: + − * / ^ ( ), sin cos tan asin acos atan sinh cosh tanh sqrt abs floor ceil round sign exp ln log pow min max, PI, E; units after a value (10 mm, 5 N / 2 mm, 9.81 m/s^2); names of other parameters. A bare number takes the parameter's unit.")
                        .small()
                        .weak(),
                );
            });
        // The Multi-Value list being edited.
        if let Some((i, mut text)) = values_edit.take() {
            let mut keep = true;
            egui::Window::new("Multi-Value List")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("Values separated by ';' (each a value or an equation):");
                    ui.add(egui::TextEdit::singleline(&mut text).desired_width(320.0));
                    ui.horizontal(|ui| {
                        if ui.button("OK").clicked() {
                            let list: Vec<String> = text
                                .split(';')
                                .map(|v| v.trim().to_owned())
                                .filter(|v| !v.is_empty())
                                .collect();
                            edits.push(Edit::Values(i, list));
                            keep = false;
                        }
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                    });
                });
            if keep {
                values_edit = Some((i, text));
            }
        }
        self.param_values_edit = values_edit;
        self.param_cols = cols;
        self.param_font = font;
        self.param_search = search_text;
        if let Some(export) = csv {
            self.open_dialog(if export {
                super::files::DialogPurpose::ExportParams
            } else {
                super::files::DialogPurpose::ImportParams
            });
        }
        self.show_params = open;
        self.param_filter = filter;
        // The If window of a row (its "if" button).
        let if_row = egui::Id::new("param_if_row");
        if let Some(n) = &if_open
            && let Some(r) = rows.iter().find(|r| &r.name == n)
        {
            super::if_builder::open(ctx, egui::Id::new(("param_if", n.as_str())), &r.equation());
            ctx.data_mut(|d| d.insert_temp(if_row, n.clone()));
        }
        let open_row: Option<String> = ctx.data(|d| d.get_temp(if_row));
        if let Some(n) = open_row {
            let id = egui::Id::new(("param_if", n.as_str()));
            match rows.iter().find(|r| r.name == n) {
                Some(r) => {
                    let def = r.def();
                    let label = r.unit_label();
                    let eval = |t: &str| {
                        crate::part::expr::eval_value(t, def, &|x| {
                            rows.iter().find(|q| q.name == x).map(|q| q.as_value())
                        })
                        .map(|(v, _)| match v {
                            crate::part::expr::Value::Num(x, _) => {
                                crate::part::params::format_in(x, def, &label)
                            }
                            crate::part::expr::Value::Bool(b) => b.to_string(),
                            crate::part::expr::Value::Text(s) => format!("\"{s}\""),
                        })
                    };
                    if let Some(t) = super::if_builder::window(ctx, id, &n, &rows, Some(&n), &eval)
                    {
                        edits.push(Edit::Expr(n.clone(), t));
                    }
                    if !super::if_builder::is_open(ctx, id) {
                        ctx.data_mut(|d| d.remove::<String>(if_row));
                    }
                }
                None => ctx.data_mut(|d| d.remove::<String>(if_row)),
            }
        }
        if edits.is_empty() {
            return;
        }
        self.checkpoint("Parameters");
        let before = self.part().clone();
        let mut err = None;
        for e in edits {
            let part = self.part_mut();
            let r = match e {
                Edit::Rename(a, b) => part.rename_parameter(&a, &b),
                Edit::Expr(n, t) => part.set_parameter_expr(&n, &t),
                Edit::Eval(i, c, ev) => {
                    let sk = &mut part.sketches[i].sketch;
                    let mut t = sk.dim_param(c).tol;
                    t.eval = ev;
                    sk.set_dimension_tolerance(c, t)
                        .map_err(|e| crate::part::PartError::Feature(e.to_string()))
                }
                Edit::Comment(owner, c) => {
                    match owner {
                        ParamOwner::Dimension { sketch, constraint } => {
                            if let Some(p) = part.sketches[sketch].sketch.dim_param_mut(constraint)
                            {
                                p.comment = c;
                            }
                        }
                        ParamOwner::User(i) => part.user_params[i].comment = c,
                        ParamOwner::Feature { .. } | ParamOwner::Physical(_) => {}
                    }
                    Ok(())
                }
                Edit::UnitSpec(i, t) => part.set_user_unit(i, &t),
                Edit::Add => {
                    part.add_user_parameter(Unit::Mm);
                    Ok(())
                }
                Edit::Delete(n) => part.delete_user_parameter(&n),
                Edit::Link(t, s) => part.link_parameter(&t, &s),
                Edit::Key(n, on) => {
                    let m = part.param_meta.entry(n.clone()).or_default();
                    m.key = on;
                    if *m == crate::part::params::ParamMeta::default() {
                        part.param_meta.remove(&n);
                    }
                    Ok(())
                }
                Edit::Precision(n, d) => {
                    let m = part.param_meta.entry(n.clone()).or_default();
                    m.precision = d;
                    if *m == crate::part::params::ParamMeta::default() {
                        part.param_meta.remove(&n);
                    }
                    Ok(())
                }
                Edit::Values(i, list) => part.set_user_values(i, list),
                Edit::Purge => {
                    part.purge_unused_parameters();
                    Ok(())
                }
            };
            if let Err(e) = r {
                err = Some(e.to_string());
            }
        }
        // Rebuild: sketches with changed dimensions, then the features.
        let part = self.part_mut();
        for ps in &mut part.sketches {
            let _ = ps.sketch.solve();
        }
        let _ = part.regenerate();
        if let Some(e) = err {
            let same =
                serde_json::to_string(self.part()).ok() == serde_json::to_string(&before).ok();
            if same {
                self.history.cancel();
            }
            self.file_status = Some(Err(format!("Parameters: {e}")));
        } else {
            self.file_status = None;
        }
        self.scene_dirty = true;
        self.solve();
    }
}

/// A feature's browser icon and its one-line description (hover text;
/// Show Extended Names in an assembly).
pub(super) fn feature_look(f: &Feature) -> (Icon, String) {
    let sm_icon = super::sheet_metal::feature_icon(f);
    match f {
        Feature::Box(b) => (
            Icon::Box,
            format!(
                "Box {:.2} x {:.2} x {:.2} mm",
                b.size[0], b.size[1], b.size[2]
            ),
        ),
        Feature::Extrude(e) => (Icon::Extrude, format!("Extrude of {}", e.sketch)),
        Feature::Revolve(r) => (Icon::Revolve, format!("Revolve of {}", r.sketch)),
        Feature::Sweep(s) => (Icon::Sweep, format!("Sweep of {}", s.sketch)),
        Feature::Coil(c) => (Icon::Coil, format!("Coil of {}", c.sketch)),
        Feature::Rib(r) => (Icon::Rib, format!("Rib of {}", r.sketch)),
        Feature::SheetFace(f) => (sm_icon, format!("Sheet metal Face of {}", f.sketch)),
        Feature::SheetOp(f) => (sm_icon, f.describe()),
        Feature::BodyOp(f) => (
            match f.op {
                crate::part::BodyOp::Thicken(_) => Icon::Thicken,
                crate::part::BodyOp::DeleteFace(_) => Icon::DeleteFace,
                crate::part::BodyOp::MoveBodies(_) => Icon::MoveBodies,
                crate::part::BodyOp::Stitch(_) => Icon::Stitch,
                crate::part::BodyOp::Patch(_) => Icon::Patch,
                crate::part::BodyOp::Trim(_) => Icon::TrimSurface,
                crate::part::BodyOp::Extend(_) => Icon::ExtendSurface,
                crate::part::BodyOp::Ruled(_) => Icon::RuledSurface,
                crate::part::BodyOp::ReplaceFace(_) => Icon::ReplaceFace,
                crate::part::BodyOp::Sculpt(_) => Icon::Sculpt,
                crate::part::BodyOp::CopyObject(_) => Icon::CopyObject,
                crate::part::BodyOp::Import(_) => Icon::Import,
            },
            f.describe(),
        ),
        Feature::SheetConvert(f) => (Icon::SheetMetal, f.describe()),
        Feature::SheetFlange(f) => (
            sm_icon,
            format!(
                "Flange on {} edge(s), {:.2} mm at {:.1}°",
                f.edges.len(),
                f.height,
                f.angle.to_degrees()
            ),
        ),
        Feature::Loft(l) => (
            Icon::Loft,
            format!("Loft through {} sections", l.sections.len()),
        ),
        Feature::Hole(h) => (
            Icon::Hole,
            format!("{} centre(s): {}", h.centers.len(), h.spec.note_ascii()),
        ),
        Feature::Thread(t) => (
            Icon::Thread,
            format!(
                "{} ({})",
                t.spec.note(),
                if t.spec.modeled {
                    "modeled"
                } else {
                    "cosmetic"
                }
            ),
        ),
        Feature::Blend(bl) => (
            match bl.kind {
                crate::part::BlendKind::Fillet { .. } => Icon::FilletEdge,
                crate::part::BlendKind::Chamfer { .. } => Icon::ChamferEdge,
            },
            format!("{} of {} edge(s)", bl.command(), bl.edges.len()),
        ),
        Feature::Draft(dr) => (
            Icon::Draft,
            format!(
                "{}, {} face(s), {:.2}°",
                dr.draft_type.label(),
                dr.faces.len(),
                dr.angle.to_degrees()
            ),
        ),
        Feature::Pattern(p) => (
            super::pattern_feature::icon_of(p),
            super::pattern_feature::summary(p),
        ),
        Feature::Shell(sh) => (
            Icon::Shell,
            format!(
                "Shell {}, {} face(s) removed",
                sh.direction.label(),
                sh.remove.len()
            ),
        ),
        Feature::Work(w) => (
            match &w.def {
                crate::part::WorkDef::Plane(_) => Icon::WorkPlane,
                crate::part::WorkDef::Axis(_) => Icon::WorkAxis,
                crate::part::WorkDef::Point(_) => Icon::WorkPoint,
                crate::part::WorkDef::Ucs(_) => Icon::Ucs,
            },
            w.def.method().to_owned(),
        ),
        Feature::Combine(c) => (
            Icon::Combine,
            format!(
                "{} of {} with {} toolbody(ies){}",
                c.operation.label(),
                c.base,
                c.tools.len(),
                if c.keep_tools { ", kept" } else { "" }
            ),
        ),
        Feature::Split(sp) => (Icon::SplitBody, sp.method.label().to_owned()),
    }
}
