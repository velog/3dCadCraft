//! The assembly's browser tree (`docs/spec/3.1-assemblies.md` §4), as in
//! Inventor: every node folds open and shut — *Relationships* (all
//! constraints), the assembly's *Origin*, and each component with its own
//! *Origin* (planes, axes, centre point), its part's work features and the
//! constraints that use it. A row of origin or work geometry lights up in
//! the view under the cursor and, while Constrain is open, a click on it is
//! a selection (the planes are not in the view, as in Inventor).

use eframe::egui::{self, Color32, RichText};

use super::CadCraftApp;
use super::icons::Icon;
use crate::assembly::relation::{ASSEMBLY, GeomRef, OriginItem};
use crate::part::Feature;

/// What a click in the tree asked for (done after drawing).
enum TreeCmd {
    Select(u32, bool),
    Open(u32),
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
        let Some(a) = self.assembly() else {
            return;
        };
        let occs: Vec<(u32, String, bool, bool, crate::document::DocRef)> = a
            .occurrences
            .iter()
            .map(|o| (o.id, o.name.clone(), o.grounded, o.visible, o.component))
            .collect();
        let all: Vec<u32> = a.constraints.iter().map(|c| c.id).collect();
        let mut cmds = Vec::new();
        self.cons.browser_hover = None;
        let asm_doc = self.part_id;
        let mut iprops = false;
        ui.horizontal(|ui| {
            super::panels::tree_icon(ui, Icon::Assembly);
            ui.label(RichText::new(name).strong()).context_menu(|ui| {
                if ui.button("iProperties...").clicked() {
                    iprops = true;
                    ui.close();
                }
            });
        });
        if iprops {
            self.open_iproperties(asm_doc);
        }
        ui.indent("asm_tree", |ui| {
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
            self.origin_folder(ui, ASSEMBLY, &mut cmds);
            for (id, label, grounded, visible, component) in occs {
                let header = fold(ui, ("asm_occ", id), false, |ui| {
                    super::panels::tree_icon(ui, Icon::Part);
                    if grounded {
                        super::panels::tree_icon(ui, Icon::Ground);
                    }
                    let mut text = RichText::new(&label);
                    if !visible {
                        text = text.weak();
                    }
                    let r = ui.selectable_label(self.asm.sel.contains(&id), text);
                    let r = if grounded {
                        r.on_hover_text("Grounded")
                    } else {
                        r
                    };
                    if r.clicked() {
                        let add = ui.input(|i| i.modifiers.command || i.modifiers.shift);
                        cmds.push(TreeCmd::Select(id, add));
                    }
                    if r.double_clicked() {
                        cmds.push(TreeCmd::Open(id));
                    }
                    if r.hovered() {
                        self.asm.hover = Some(id);
                    }
                    r.context_menu(|ui| {
                        if !self.asm.sel.contains(&id) {
                            self.asm.sel = vec![id];
                        }
                        let ids = self.asm.sel.clone();
                        self.occurrence_menu(ui, &ids);
                    });
                });
                header.body(|ui| {
                    self.origin_folder(ui, id, &mut cmds);
                    // The part's work features.
                    let works = self
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
                            occurrence: id,
                            id: wid,
                        };
                        self.geom_row(ui, Icon::WorkPlane, &wname, g, &mut cmds);
                    }
                    // Its constraints.
                    let uses: Vec<u32> = self
                        .assembly()
                        .map(|a| {
                            a.constraints
                                .iter()
                                .filter(|c| c.uses(id))
                                .map(|c| c.id)
                                .collect()
                        })
                        .unwrap_or_default();
                    for c in uses {
                        self.constraint_row(ui, c, &mut cmds);
                    }
                });
            }
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
                TreeCmd::Open(id) => self.open_component(id),
                TreeCmd::Pick(g) => {
                    if self.cons.draft.is_some() {
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
                TreeCmd::DeleteConstraint(id) => self.delete_constraint(id),
                TreeCmd::Drive(id) => self.open_drive(id),
            }
        }
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
                let g = GeomRef::Origin { occurrence, item };
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

    /// One constraint (red when sick, grey when suppressed).
    fn constraint_row(&mut self, ui: &mut egui::Ui, id: u32, cmds: &mut Vec<TreeCmd>) {
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
                a.find(r.occurrence())
                    .map_or("?".to_owned(), |o| o.name.clone())
            }
        };
        let label = format!("{} ({}, {})", c.name, between(&c.a), between(&c.b));
        let suppressed = c.suppressed;
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
            if suppressed {
                text = text.weak();
            } else if sick.is_some() {
                text = text.color(Color32::from_rgb(235, 80, 70));
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
                if ui.button("Delete").clicked() {
                    cmds.push(TreeCmd::DeleteConstraint(id));
                    ui.close();
                }
            });
        });
    }
}

/// A node that folds open and shut (its state remembered by egui).
fn fold<'a>(
    ui: &'a mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    open: bool,
    header: impl FnOnce(&mut egui::Ui),
) -> egui::collapsing_header::HeaderResponse<'a, ()> {
    egui::collapsing_header::CollapsingState::load_with_default_open(
        ui.ctx(),
        ui.make_persistent_id(id),
        open,
    )
    .show_header(ui, header)
}
