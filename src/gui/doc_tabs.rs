//! Document tabs (`docs/spec/0.6-files.md` §3.1): every open file has a
//! tab above the view. The active document lives in the app's own fields
//! (`project`, `part_id`, `history`, `file_path`, `saved_sig`, camera,
//! selection); the others wait in `tabs` as [`DocTab`]s, swapped in on a
//! switch. Leaving a document finishes its sketch edit and cancels its
//! open command, so no draft ever points into another document.

use eframe::egui::{self, Color32, RichText};

use super::files::Pending;
use super::{CadCraftApp, Selection};
use crate::command::History;
use crate::document::{DocumentId, Project};

/// A document that is open but not active.
pub(super) struct DocTab {
    pub project: Project,
    pub part_id: DocumentId,
    pub history: History<Project>,
    pub file_path: String,
    pub saved_sig: u64,
    pub camera: super::camera::OrbitCamera,
    pub selection: Selection,
    /// Unsaved changes (an inactive document cannot change).
    pub dirty: bool,
    /// Its title (file name or project name).
    pub title: String,
}

impl CadCraftApp {
    /// How many documents are open.
    pub(super) fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    /// Ends what is open in the active document (before it is left).
    pub(super) fn leave_document(&mut self) {
        self.finish_sketch();
        self.cancel_hole();
        self.cancel_extrude();
        self.blend_draft = None;
        self.work_draft = None;
        self.measure_pick = None;
        self.face_sel = None;
        self.edge_sel = None;
        self.sel.clear();
        self.prev_sel.clear();
        self.select_other = None;
        self.mode = super::Mode::Normal;
    }

    /// The active document taken out of the app's fields.
    fn stash_active(&mut self) -> DocTab {
        let now = f64::INFINITY; // force the dirty check
        let dirty = self.is_dirty(now, true);
        DocTab {
            title: self.document_title(),
            project: std::mem::replace(&mut self.project, Project::new("Untitled")),
            part_id: self.part_id,
            history: std::mem::take(&mut self.history),
            file_path: std::mem::take(&mut self.file_path),
            saved_sig: self.saved_sig,
            camera: self.camera.clone(),
            selection: self.selection,
            dirty,
        }
    }

    /// Makes `t` the active document.
    fn load_tab(&mut self, t: DocTab) {
        self.project = t.project;
        self.part_id = t.part_id;
        self.asm = Default::default();
        self.cons = Default::default();
        self.history = t.history;
        self.file_path = t.file_path;
        self.asm_refresh_status();
        self.saved_sig = t.saved_sig;
        self.camera = t.camera;
        self.selection = t.selection;
        self.editing = None;
        self.dirty_cache = (f64::NEG_INFINITY, false);
        *self.extrude_preview.borrow_mut() = None;
        self.after_restore();
    }

    /// Switches to tab `k`.
    pub(super) fn switch_tab(&mut self, k: usize) {
        if k == self.active_tab || k >= self.tabs.len() {
            return;
        }
        self.leave_document();
        let cur = self.stash_active();
        self.tabs[self.active_tab] = Some(cur);
        if let Some(t) = self.tabs[k].take() {
            self.active_tab = k;
            self.load_tab(t);
        }
    }

    /// The active document is an untouched empty "Untitled": New / Open
    /// replace it instead of adding a tab.
    fn active_is_blank(&mut self) -> bool {
        self.file_path.trim().is_empty()
            && !self.is_dirty(f64::INFINITY, true)
            && self.part().features.is_empty()
            && self.part().sketches.is_empty()
    }

    /// A new, empty tab becomes active (the old document stays open).
    pub(super) fn add_blank_tab(&mut self) {
        if self.active_is_blank() {
            return;
        }
        self.leave_document();
        let cur = self.stash_active();
        self.tabs[self.active_tab] = Some(cur);
        self.tabs.push(None);
        self.active_tab = self.tabs.len() - 1;
        self.history = History::default();
    }

    /// The tab whose file is `path`, if open.
    pub(super) fn tab_of_path(&self, path: &std::path::Path) -> Option<usize> {
        let same = |p: &str| !p.trim().is_empty() && std::path::Path::new(p.trim()) == path;
        if same(&self.file_path) {
            return Some(self.active_tab);
        }
        self.tabs
            .iter()
            .position(|t| t.as_ref().is_some_and(|t| same(&t.file_path)))
    }

    /// Closes the active tab without asking (the last one becomes an empty
    /// Untitled).
    pub(super) fn close_active_tab(&mut self) {
        if self.tabs.len() <= 1 {
            self.new_project();
            return;
        }
        self.leave_document();
        self.tabs.remove(self.active_tab);
        let k = self.active_tab.min(self.tabs.len() - 1);
        if let Some(t) = self.tabs[k].take() {
            self.active_tab = k;
            self.load_tab(t);
        }
    }

    /// Close Tab `k`: a changed document asks first (it comes forward).
    pub(super) fn request_close_tab(&mut self, k: usize, now: f64) {
        self.switch_tab(k);
        if self.is_dirty(now, true) {
            self.unsaved_prompt = Some(Pending::CloseTab);
        } else {
            self.close_active_tab();
        }
    }

    /// An inactive tab with unsaved changes the quit has not been told to
    /// drop yet.
    pub(super) fn dirty_other_tab(&self) -> Option<usize> {
        (0..self.tabs.len()).find(|&k| {
            k != self.active_tab
                && !self.quit_dropped.contains(&k)
                && self.tabs[k].as_ref().is_some_and(|t| t.dirty)
        })
    }

    /// The tab bar above the view.
    pub(super) fn tab_bar(&mut self, ui: &mut egui::Ui) {
        let now = ui.input(|i| i.time);
        let active_dirty = self.is_dirty(now, false);
        let mut switch = None;
        let mut close = None;
        let mut close_others = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for k in 0..self.tabs.len() {
                let (title, dirty) = match &self.tabs[k] {
                    Some(t) => (t.title.clone(), t.dirty),
                    None => (self.document_title(), active_dirty),
                };
                let active = k == self.active_tab;
                let text = format!("{title}{}", if dirty { " *" } else { "" });
                let mut rt = RichText::new(text);
                if active {
                    rt = rt.strong();
                } else {
                    rt = rt.color(Color32::GRAY);
                }
                let r = ui.selectable_label(active, rt);
                if r.clicked() {
                    switch = Some(k);
                }
                r.context_menu(|ui| {
                    if ui.button("Close").clicked() {
                        close = Some(k);
                        ui.close();
                    }
                    if ui.button("Close Other").clicked() {
                        switch = Some(k);
                        close_others = true;
                        ui.close();
                    }
                });
                if ui
                    .small_button("x")
                    .on_hover_text("Close this document")
                    .clicked()
                {
                    close = Some(k);
                }
                ui.separator();
            }
            if ui
                .small_button("+")
                .on_hover_text("New document (Ctrl+N)")
                .clicked()
            {
                self.add_blank_tab();
                self.new_project();
            }
        });
        if let Some(k) = switch {
            self.switch_tab(k);
        }
        if close_others {
            // Unchanged ones close now; a changed one keeps its tab.
            let keep = self.active_tab;
            let mut k = self.tabs.len();
            while k > 0 {
                k -= 1;
                if k != keep && self.tabs[k].as_ref().is_some_and(|t| !t.dirty) {
                    self.tabs.remove(k);
                    if k < self.active_tab {
                        self.active_tab -= 1;
                    }
                }
            }
        }
        if let Some(k) = close {
            self.request_close_tab(k, now);
        }
    }

    /// Ctrl+Tab / Ctrl+Shift+Tab: the next / previous tab.
    pub(super) fn tab_keys(&mut self, ctx: &egui::Context) {
        let n = self.tabs.len();
        if n < 2 {
            self.tab_step = 0;
            return;
        }
        // Taken from the raw input (`raw_input_hook`) before egui moves
        // its widget focus with Tab.
        let _ = ctx;
        let step = std::mem::take(&mut self.tab_step);
        if step != 0 {
            let k = (self.active_tab as i64 + step as i64).rem_euclid(n as i64) as usize;
            self.switch_tab(k);
        }
    }
}
