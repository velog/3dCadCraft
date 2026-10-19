//! History editing from the Model Browser (`docs/spec/2.16-browser.md`):
//! End of Part, reorder by drag, Suppress / Unsuppress, Delete with
//! options, Rename. The part-side logic is in `part::history`.

use eframe::egui::{self, Color32, Key, RichText};

use super::{CadCraftApp, Selection};
use crate::part::history::DeleteOptions;

/// A browser row being renamed, by persistent id (indices change).
#[derive(Clone, Debug, PartialEq)]
pub(super) enum RenameTarget {
    Feature(String),
    Sketch(String),
}

/// What is dragged in the browser.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum BrowserDrag {
    /// Feature by index (reorder).
    Feature(usize),
    /// The End of Part marker.
    Eop,
}

/// The text buffer of the rename field (egui memory, so the browser can
/// draw it with `&self`): text and whether focus was requested.
pub(super) fn rename_buffer_id() -> egui::Id {
    egui::Id::new("browser_rename_buffer")
}

impl CadCraftApp {
    /// Starts renaming a row: the field shows the current name.
    pub(super) fn start_rename(&mut self, ctx: &egui::Context, target: RenameTarget) {
        let part = self.part();
        let name = match &target {
            RenameTarget::Feature(id) => part
                .features
                .iter()
                .find(|f| f.id() == id)
                .map(|f| f.name().to_owned()),
            RenameTarget::Sketch(id) => part
                .sketches
                .iter()
                .find(|s| s.id == *id)
                .map(|s| s.name.clone()),
        };
        let Some(name) = name else {
            return;
        };
        ctx.data_mut(|d| d.insert_temp(rename_buffer_id(), (name, false)));
        self.rename = Some(target);
    }

    /// Applies a rename (display name only, rule 3).
    pub(super) fn apply_rename(&mut self, target: RenameTarget, name: String) {
        self.rename = None;
        let part = self.part();
        let (index, old) = match &target {
            RenameTarget::Feature(id) => (
                part.feature_index(id),
                part.features
                    .iter()
                    .find(|f| f.id() == id)
                    .map(|f| f.name().to_owned()),
            ),
            RenameTarget::Sketch(id) => (
                part.sketches.iter().position(|s| s.id == *id),
                part.sketches
                    .iter()
                    .find(|s| s.id == *id)
                    .map(|s| s.name.clone()),
            ),
        };
        let Some(i) = index else {
            return;
        };
        if old.as_deref() == Some(name.trim()) {
            return;
        }
        self.checkpoint("Rename");
        let r = match target {
            RenameTarget::Feature(_) => self.part_mut().rename_feature(i, &name),
            RenameTarget::Sketch(_) => self.part_mut().rename_sketch(i, &name),
        };
        if let Err(e) = r {
            self.history.cancel();
            self.file_status = Some(Err(format!("Rename: {e}")));
        }
    }

    /// Suppress Features / Unsuppress Features on feature `k`.
    pub(super) fn set_suppressed(&mut self, k: usize, on: bool) {
        let cmd = if on {
            "Suppress Features"
        } else {
            "Unsuppress Features"
        };
        self.checkpoint(cmd);
        let r = if on {
            self.part_mut().suppress_feature(k)
        } else {
            self.part_mut().unsuppress_feature(k)
        };
        match r {
            Ok(ids) => {
                self.file_status = Some(Ok(format!("{cmd}: {}", ids.join(", "))));
            }
            Err(e) => {
                self.history.cancel();
                self.file_status = Some(Err(format!("{cmd}: {e}")));
            }
        }
        self.after_history_edit();
    }

    /// Move EOP to Top (0) / End (`features.len()`) / Here (`k + 1`).
    pub(super) fn move_eop(&mut self, at: usize) {
        if at.min(self.part().features.len()) == self.part().eop_index() {
            return;
        }
        self.finish_sketch();
        self.checkpoint("Move End of Part");
        if let Err(e) = self.part_mut().set_eop(at) {
            self.history.cancel();
            self.file_status = Some(Err(format!("Move End of Part: {e}")));
        }
        self.after_history_edit();
    }

    /// Delete All Features Below EOP.
    pub(super) fn delete_below_eop(&mut self) {
        self.finish_sketch();
        self.checkpoint("Delete All Features Below EOP");
        match self.part_mut().delete_below_eop() {
            Ok(names) => {
                self.file_status = Some(Ok(format!("Deleted: {}", names.join(", "))));
            }
            Err(e) => {
                self.history.cancel();
                self.file_status = Some(Err(format!("Delete All Features Below EOP: {e}")));
            }
        }
        self.selection = Selection::Body;
        self.after_history_edit();
    }

    /// Reorder (browser drag): feature `from` to index `to`.
    pub(super) fn move_feature(&mut self, from: usize, to: usize) {
        if from == to {
            return;
        }
        self.finish_sketch();
        let name = self
            .part()
            .features
            .get(from)
            .map(|f| f.id().to_owned())
            .unwrap_or_default();
        self.checkpoint("Reorder");
        match self.part_mut().move_feature(from, to) {
            Ok(()) => {
                self.selection = match self.part().feature_index(&name) {
                    Some(k) => Selection::Feature(k),
                    None => Selection::Body,
                };
                self.file_status = None;
            }
            Err(e) => {
                self.history.cancel();
                self.file_status = Some(Err(format!("Reorder: {e}")));
            }
        }
        self.after_history_edit();
    }

    /// The part changed its history: selections by index may be stale.
    fn after_history_edit(&mut self) {
        self.sel.clear();
        self.edge_cache.replace(None);
        self.scene_dirty = true;
        if let Selection::Feature(k) = self.selection
            && k >= self.part().features.len()
        {
            self.selection = Selection::Body;
        }
    }

    /// Delete Feature: asks first when it takes more than the feature (its
    /// sketch, dependants) or breaks something.
    pub(super) fn delete_feature(&mut self, k: usize) {
        if k >= self.part().features.len() {
            return;
        }
        let part = self.part();
        let plan = part.delete_plan(k, DeleteOptions::default());
        let hit = part.breaks_if_removed(k);
        if plan.features.len() <= 1 && plan.sketches.is_empty() && hit.is_empty() {
            self.delete_feature_now(k, DeleteOptions::default());
        } else {
            self.delete_opts = DeleteOptions::default();
            self.confirm_delete = Some((k, hit));
        }
    }

    /// The Delete Features dialog: what goes, with Inventor's options.
    pub(super) fn confirm_delete_window(&mut self, ctx: &egui::Context) {
        let Some((k, hit)) = self.confirm_delete.clone() else {
            return;
        };
        let Some(name) = self.part().features.get(k).map(|f| f.name().to_owned()) else {
            self.confirm_delete = None;
            return;
        };
        let mut opts = self.delete_opts;
        let plan = self.part().delete_plan(k, opts);
        let part = self.part();
        let feats: Vec<String> = plan
            .features
            .iter()
            .filter(|&&j| j != k)
            .map(|&j| part.features[j].name().to_owned())
            .collect();
        let sketches: Vec<String> = plan
            .sketches
            .iter()
            .map(|&i| part.sketches[i].name.clone())
            .collect();
        let mut answer = None;
        egui::Window::new("Delete Features")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label(format!("Delete {name}"));
                ui.add_space(4.0);
                ui.checkbox(
                    &mut opts.consumed_sketches,
                    "Consumed sketches and features",
                )
                .on_hover_text("Also delete the sketches only these features use");
                ui.checkbox(&mut opts.dependents, "Dependent sketches and features")
                    .on_hover_text(
                        "Also delete what references it; kept ones become sick (Warning)",
                    );
                if !feats.is_empty() || !sketches.is_empty() {
                    ui.add_space(4.0);
                    ui.label(RichText::new("Also deleted:").small());
                    for n in feats.iter().chain(&sketches) {
                        ui.label(RichText::new(format!("  {n}")).small());
                    }
                }
                if !opts.dependents && !hit.is_empty() {
                    ui.add_space(4.0);
                    ui.label(RichText::new("Kept, they become sick:").small());
                    for (what, why) in &hit {
                        ui.horizontal(|ui| {
                            ui.colored_label(Color32::from_rgb(230, 140, 0), "!");
                            ui.label(RichText::new(what).strong());
                            ui.label(RichText::new(why).small().weak());
                        });
                    }
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        answer = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        answer = Some(false);
                    }
                });
            });
        self.delete_opts = opts;
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            answer = Some(false);
        }
        match answer {
            Some(true) => {
                self.confirm_delete = None;
                self.delete_feature_now(k, opts);
            }
            Some(false) => self.confirm_delete = None,
            None => {}
        }
    }

    fn delete_feature_now(&mut self, k: usize, opts: DeleteOptions) {
        if k >= self.part().features.len() {
            return;
        }
        self.finish_sketch();
        self.checkpoint("Delete Feature");
        match self.part_mut().delete_features(k, opts) {
            Ok(_) => self.file_status = None,
            Err(e) => {
                self.history.cancel();
                self.file_status = Some(Err(format!("Delete: {e}")));
            }
        }
        self.selection = Selection::Body;
        self.after_history_edit();
    }
}
