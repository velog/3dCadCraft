//! Document tabs (`docs/spec/0.6-files.md` §3.1): every open document has a
//! tab above the view. The active document lives in the app's own fields
//! (`project`, `part_id`, `history`, `file_path`, `saved_sig`, camera,
//! selection); the others wait in `tabs` as [`DocTab`]s, swapped in on a
//! switch. Leaving a document finishes its sketch edit and cancels its
//! open command, so no draft ever points into another document.
//!
//! Several tabs may show documents of **one file** (an assembly and a part of
//! its design file, Inventor's component opened in its own window): they form
//! a *group* and share one project, Undo history and saved state. Exactly one
//! place holds that project — the app's fields when the active tab is of the
//! group, else the group's *holder* tab; the others keep only their view
//! (document, camera, selection). A change made in one tab is in the project
//! the others show, so an assembly shows its part as edited in the part's tab.

use eframe::egui::{self, RichText};

use super::files::Pending;
use super::{CadCraftApp, Selection};
use crate::command::History;
use crate::document::{DocumentId, Project};

/// A document that is open but not active.
pub(super) struct DocTab {
    /// The file's project (only in the group's holder; an empty stand-in in
    /// the others).
    pub project: Project,
    pub part_id: DocumentId,
    pub history: History<Project>,
    pub file_path: String,
    pub saved_sig: u64,
    pub camera: super::camera::OrbitCamera,
    pub selection: Selection,
    /// Unsaved changes (an inactive document cannot change); only the holder's
    /// counts.
    pub dirty: bool,
    /// Its title (file name or project name).
    pub title: String,
    /// The open file it belongs to: tabs of one group share the project.
    pub group: u64,
    /// This tab holds the group's project, Undo and saved state.
    pub holds: bool,
    /// The document's name when the file holds several (shown after the
    /// title), else empty.
    pub doc_name: String,
}

/// The tab holding group `g`'s project, if it is in `tabs` (not active).
pub(super) fn holder_of(tabs: &[Option<DocTab>], g: u64) -> Option<usize> {
    tabs.iter()
        .position(|t| t.as_ref().is_some_and(|t| t.holds && t.group == g))
}

/// Tab `k` takes its group's project from the group's holder (it becomes the
/// holder). Nothing happens when it holds it already or no tab does.
pub(super) fn take_holding(tabs: &mut [Option<DocTab>], k: usize) {
    let Some(g) = tabs
        .get(k)
        .and_then(|t| t.as_ref())
        .filter(|t| !t.holds)
        .map(|t| t.group)
    else {
        return;
    };
    let Some(h) = holder_of(tabs, g) else {
        return;
    };
    let Some(from) = tabs[h].as_mut() else {
        return;
    };
    from.holds = false;
    let project = std::mem::replace(&mut from.project, Project::new("Untitled"));
    let history = std::mem::take(&mut from.history);
    let dirty = std::mem::take(&mut from.dirty);
    let (saved, path) = (from.saved_sig, from.file_path.clone());
    if let Some(to) = tabs[k].as_mut() {
        to.project = project;
        to.history = history;
        to.dirty = dirty;
        to.saved_sig = saved;
        to.file_path = path;
        to.holds = true;
    }
}

/// Which tabs Close Other closes when tab `keep` of group `g` stays: the
/// other tabs of its file, and every file without unsaved changes.
pub(super) fn close_other_set(tabs: &[Option<DocTab>], keep: usize, g: u64) -> Vec<bool> {
    (0..tabs.len())
        .map(|k| {
            k != keep
                && tabs[k]
                    .as_ref()
                    .is_some_and(|t| t.group == g || !group_dirty(tabs, t.group))
        })
        .collect()
}

/// Whether group `g` (not the active one) has unsaved changes.
pub(super) fn group_dirty(tabs: &[Option<DocTab>], g: u64) -> bool {
    holder_of(tabs, g)
        .and_then(|h| tabs[h].as_ref())
        .is_some_and(|t| t.dirty)
}

impl CadCraftApp {
    /// How many documents are open.
    pub(super) fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    /// Ends what is open in the active document (before it is left).
    pub(super) fn leave_document(&mut self) {
        // A frame joint's preview goes back first (it changed the project).
        self.joint_cancel();
        self.text_draft = None;
        self.dxf_draft = None;
        self.finish_sketch();
        self.cancel_hole();
        self.cancel_extrude();
        self.blend_draft = None;
        self.work_draft = None;
        self.part_planned = None;
        self.thicken_draft = None;
        self.ff_edit = None;
        self.delete_draft = None;
        self.move_draft = None;
        self.section = None;
        // A sheet metal command points into this document's part.
        if self.sm.draft.take().is_some() {
            self.end_feature_edit();
        }
        self.sm.flat = false;
        self.measure_pick = None;
        self.face_sel = None;
        self.edge_sel = None;
        self.sel.clear();
        self.prev_sel.clear();
        self.select_other = None;
        self.mode = super::Mode::Normal;
        self.drw.reset();
    }

    /// The name of the active document when its file holds several.
    fn doc_label(&self) -> String {
        if self.project.documents().len() < 2 {
            return String::new();
        }
        self.project
            .document(self.part_id)
            .map(|d| d.name.clone())
            .unwrap_or_default()
    }

    /// The active document taken out of the app's fields (with its file's
    /// project: it becomes the group's holder).
    fn stash_active(&mut self) -> DocTab {
        let now = f64::INFINITY; // force the dirty check
        let dirty = self.is_dirty(now, true);
        DocTab {
            title: self.document_title(),
            doc_name: self.doc_label(),
            project: std::mem::replace(&mut self.project, Project::new("Untitled")),
            part_id: self.part_id,
            history: std::mem::take(&mut self.history),
            file_path: std::mem::take(&mut self.file_path),
            saved_sig: self.saved_sig,
            camera: self.camera.clone(),
            selection: self.selection,
            dirty,
            group: self.tab_group,
            holds: true,
        }
    }

    /// The active view only (another tab of the same file becomes active and
    /// keeps the project in the app's fields).
    fn stash_view(&self) -> DocTab {
        DocTab {
            title: self.document_title(),
            doc_name: self.doc_label(),
            project: Project::new("Untitled"),
            part_id: self.part_id,
            history: History::default(),
            file_path: self.file_path.clone(),
            saved_sig: self.saved_sig,
            camera: self.camera.clone(),
            selection: self.selection,
            dirty: false,
            group: self.tab_group,
            holds: false,
        }
    }

    /// Makes `t` (its group's holder) the active document.
    fn load_tab(&mut self, t: DocTab) {
        self.project = t.project;
        self.part_id = t.part_id;
        self.tab_group = t.group;
        self.reset_asm_ui();
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
        // A part of the file, or a file open in another tab, may have
        // changed meanwhile.
        if self.is_assembly() || self.is_drawing() {
            self.document_shown_again();
        }
    }

    /// Makes view `t` of the active file the active one: its document comes
    /// forward with what changed in the other tabs of the file.
    fn load_view(&mut self, t: DocTab) {
        self.reset_asm_ui();
        self.cons = Default::default();
        if self.project.document(t.part_id).is_none() {
            // Undo or Delete Document took it out of the file.
            self.file_status = Some(Err(format!(
                "{}: the document is no longer in this file",
                t.doc_name
            )));
            self.scene_dirty = true;
            return;
        }
        if t.part_id == self.part_id {
            self.scene_dirty = true;
        } else {
            self.activate_document(t.part_id);
        }
        if self.is_assembly() || self.is_drawing() {
            self.document_shown_again();
        }
        self.camera = t.camera;
        self.selection = t.selection;
        self.after_restore();
    }

    /// An assembly or drawing comes forward again: what it shows of the
    /// file's parts (and of files open in other tabs) is followed.
    fn document_shown_again(&mut self) {
        self.share_open_tabs();
        if self.is_assembly() {
            self.follow_patterns();
            self.follow_frames();
            self.bolts_follow(false);
            self.make_follow();
            self.derive_follow();
            // Cross-part references follow the parts edited meanwhile.
            self.checkpoint("Update");
            if !self.context_follow() {
                self.history.cancel();
            }
            self.asm_refresh_status();
        }
        self.scene_dirty = true;
    }

    /// Edit in place of a part of this file ends before its tab is left (the
    /// tab keeps the assembly, not the part out of its context).
    fn end_local_in_place(&mut self) {
        if self.in_place_now().is_some_and(|ip| ip.linked.is_none()) {
            self.return_to_assembly();
        }
    }

    /// Switches to tab `k`.
    pub(super) fn switch_tab(&mut self, k: usize) {
        if k == self.active_tab || k >= self.tabs.len() {
            return;
        }
        // The part being built stays in front (spec 0.19 §3.3).
        if self.refuse_while_building("Switch tab") {
            return;
        }
        let Some(group) = self.tabs[k].as_ref().map(|t| t.group) else {
            return;
        };
        self.end_local_in_place();
        self.leave_document();
        if group == self.tab_group {
            // Another document of the same file: the project stays.
            let view = self.stash_view();
            self.tabs[self.active_tab] = Some(view);
            if let Some(t) = self.tabs[k].take() {
                self.active_tab = k;
                self.load_view(t);
            }
            return;
        }
        let cur = self.stash_active();
        self.tabs[self.active_tab] = Some(cur);
        take_holding(&mut self.tabs, k);
        if let Some(t) = self.tabs[k].take() {
            self.active_tab = k;
            self.load_tab(t);
        }
    }

    /// Opens document `id` of the active file in a tab of its own (Inventor:
    /// Open of a component opens its own window; the assembly stays open in
    /// its tab). A tab already showing it comes forward.
    pub(super) fn open_document_in_tab(&mut self, id: DocumentId) {
        if id == self.part_id || self.project.document(id).is_none() {
            return;
        }
        let g = self.tab_group;
        if let Some(k) = (0..self.tabs.len()).find(|&k| {
            self.tabs[k]
                .as_ref()
                .is_some_and(|t| t.group == g && t.part_id == id)
        }) {
            self.switch_tab(k);
            return;
        }
        self.end_local_in_place();
        self.leave_document();
        let view = self.stash_view();
        self.tabs[self.active_tab] = Some(view);
        self.active_tab += 1;
        self.tabs.insert(self.active_tab, None);
        self.reset_asm_ui();
        self.cons = Default::default();
        self.activate_document(id);
    }

    /// The tabs of the active file other than the active one.
    fn active_siblings(&self) -> Vec<usize> {
        let g = self.tab_group;
        (0..self.tabs.len())
            .filter(|&k| {
                k != self.active_tab && self.tabs[k].as_ref().is_some_and(|t| t.group == g)
            })
            .collect()
    }

    /// Other tabs of the active file whose document left it (Undo, Delete
    /// Document, Save to Separate File) close: nothing is lost, the file
    /// stays open here.
    pub(super) fn prune_group_tabs(&mut self) {
        let g = self.tab_group;
        let mut k = self.tabs.len();
        while k > 0 {
            k -= 1;
            let gone = k != self.active_tab
                && self.tabs[k].as_ref().is_some_and(|t| {
                    t.group == g && !t.holds && self.project.document(t.part_id).is_none()
                });
            if gone {
                self.tabs.remove(k);
                if k < self.active_tab {
                    self.active_tab -= 1;
                }
            }
        }
    }

    /// After Save / Save As: the other tabs of the file show its new path.
    pub(super) fn sync_group_paths(&mut self) {
        let (g, path, title) = (
            self.tab_group,
            self.file_path.clone(),
            self.document_title(),
        );
        for t in self.tabs.iter_mut().flatten() {
            if t.group == g {
                t.file_path = path.clone();
                t.title = title.clone();
            }
        }
    }

    /// The active document is an untouched empty "Untitled": New / Open
    /// replace it instead of adding a tab.
    pub(super) fn active_is_blank(&mut self) -> bool {
        // A new drawing or assembly is no blank: Home hid its tab and the next
        // New replaced it.
        let part = self
            .project
            .document(self.part_id)
            .is_none_or(|d| matches!(d.data, crate::document::DocumentData::Part(_)));
        part && self.file_path.trim().is_empty()
            && self.active_siblings().is_empty()
            && !self.is_dirty(f64::INFINITY, true)
            && self.part().features.is_empty()
            && self.part().sketches.is_empty()
    }

    /// A new, empty tab becomes active (the old document stays open).
    pub(super) fn add_blank_tab(&mut self) {
        if self.active_is_blank() {
            return;
        }
        self.end_local_in_place();
        self.leave_document();
        let cur = self.stash_active();
        self.tabs[self.active_tab] = Some(cur);
        self.tabs.push(None);
        self.active_tab = self.tabs.len() - 1;
        self.tab_group = self.next_tab_group;
        self.next_tab_group += 1;
        self.history = History::default();
    }

    /// The tab whose file is `path`, if open (the first one when several
    /// tabs show documents of it).
    pub(super) fn tab_of_path(&self, path: &std::path::Path) -> Option<usize> {
        let canon = path.canonicalize().ok();
        let same = |p: &str| {
            let p = p.trim();
            !p.is_empty()
                && (std::path::Path::new(p) == path
                    || canon.as_ref().is_some_and(|c| {
                        std::path::Path::new(p)
                            .canonicalize()
                            .is_ok_and(|q| q == *c)
                    }))
        };
        if same(&self.file_path) {
            return Some(self.active_tab);
        }
        self.tabs
            .iter()
            .position(|t| t.as_ref().is_some_and(|t| same(&t.file_path)))
    }

    /// Whether file `path` is open in a tab of another file (Save As or
    /// Save to Separate File there would leave that tab saving over it).
    pub(super) fn open_in_other_tab(&self, path: &std::path::Path) -> bool {
        self.tab_of_path(path).is_some_and(|k| {
            self.tabs[k]
                .as_ref()
                .is_some_and(|t| t.group != self.tab_group)
        })
    }

    /// Closes the active tab without asking (the last one becomes an empty
    /// Untitled). Another tab of the same file keeps the file open.
    pub(super) fn close_active_tab(&mut self) {
        if let Some(&s) = self.active_siblings().first() {
            self.end_local_in_place();
            self.leave_document();
            self.tabs.remove(self.active_tab);
            let s = if s > self.active_tab { s - 1 } else { s };
            if let Some(t) = self.tabs[s].take() {
                self.active_tab = s;
                self.load_view(t);
            }
            return;
        }
        if self.tabs.len() <= 1 {
            self.new_project();
            // Inventor: Home comes back when the last document closes.
            if self.home.s.show_on_start {
                self.show_home();
            }
            return;
        }
        self.leave_document();
        // Changes it kept unsaved are no longer what assemblies show.
        if !self.file_path.trim().is_empty() {
            self.asm
                .libs
                .forget(std::path::Path::new(self.file_path.trim()));
        }
        self.tabs.remove(self.active_tab);
        let k = self.active_tab.min(self.tabs.len() - 1);
        take_holding(&mut self.tabs, k);
        if let Some(t) = self.tabs[k].take() {
            self.active_tab = k;
            self.load_tab(t);
        }
    }

    /// Close Tab `k`: a changed document asks first (it comes forward),
    /// unless another tab still shows its file.
    pub(super) fn request_close_tab(&mut self, k: usize, now: f64) {
        self.switch_tab(k);
        if self.active_siblings().is_empty() && self.is_dirty(now, true) {
            self.unsaved_prompt = Some(Pending::CloseTab);
        } else {
            self.close_active_tab();
        }
    }

    /// An inactive tab with unsaved changes the quit has not been told to
    /// drop yet (one per file: its holder).
    pub(super) fn dirty_other_tab(&self) -> Option<usize> {
        (0..self.tabs.len()).find(|&k| {
            k != self.active_tab
                && !self.quit_dropped.contains(&k)
                && self.tabs[k]
                    .as_ref()
                    .is_some_and(|t| t.holds && t.dirty && t.group != self.tab_group)
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
            self.home_tab(ui);
            // Home over an untouched Untitled: Home alone, as at Inventor's start.
            let only_home = self.home.shown && self.tabs.len() == 1 && self.active_is_blank();
            for k in 0..if only_home { 0 } else { self.tabs.len() } {
                let (title, dirty) = match &self.tabs[k] {
                    Some(t) => {
                        let dirty = if t.group == self.tab_group {
                            active_dirty
                        } else {
                            group_dirty(&self.tabs, t.group)
                        };
                        let title = if t.doc_name.is_empty() {
                            t.title.clone()
                        } else {
                            format!("{} › {}", t.title, t.doc_name)
                        };
                        (title, dirty)
                    }
                    None => (self.document_title(), active_dirty),
                };
                let active = k == self.active_tab && !self.home.shown;
                let text = format!("{title}{}", if dirty { " *" } else { "" });
                let mut rt = RichText::new(text);
                if active {
                    rt = rt.strong();
                } else {
                    rt = rt.color(super::theme::tokens().text_weak);
                }
                let r = ui.selectable_label(active, rt);
                if r.clicked() {
                    switch = Some(k);
                }
                // The active file's documents (spec 0.6b §3.3).
                if active {
                    self.active_document_menu(ui);
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
                if super::theme::glyph(ui, super::theme::Glyph::Close)
                    .on_hover_text("Close this document")
                    .clicked()
                {
                    close = Some(k);
                }
                ui.separator();
            }
            if super::theme::glyph(ui, super::theme::Glyph::Plus)
                .on_hover_text("New document (Ctrl+N)")
                .clicked()
            {
                self.add_blank_tab();
                self.new_project();
            }
        });
        if let Some(k) = switch {
            self.home.shown = false;
            self.switch_tab(k);
        }
        if close_others {
            // Unchanged files close now; a changed one keeps its tabs. The
            // kept file's other tabs close too (the file stays open here).
            let closes = close_other_set(&self.tabs, self.active_tab, self.tab_group);
            let mut k = self.tabs.len();
            while k > 0 {
                k -= 1;
                if closes[k] {
                    if let Some(t) = self.tabs.remove(k)
                        && t.holds
                        && !t.file_path.trim().is_empty()
                    {
                        self.asm
                            .libs
                            .forget(std::path::Path::new(t.file_path.trim()));
                    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::DocumentData;
    use crate::part::PartDocument;

    fn tab(project: Project, part_id: DocumentId, group: u64, holds: bool, dirty: bool) -> DocTab {
        DocTab {
            project,
            part_id,
            history: History::default(),
            file_path: format!("f{group}.3dcadcraft"),
            saved_sig: 7,
            camera: Default::default(),
            selection: Selection::Body,
            dirty,
            title: format!("f{group}"),
            group,
            holds,
            doc_name: String::new(),
        }
    }

    /// An assembly and its part open in two tabs of one file: the part's tab
    /// takes the project (with its changes) from the assembly's tab, so both
    /// show one project and nothing is copied or lost.
    #[test]
    fn a_tab_of_the_same_file_takes_the_project_from_its_holder() {
        let mut p = Project::new("Design");
        let part = p.add_document("Part1", DocumentData::Part(PartDocument::default()));
        let asm = p.add_document("Asm", DocumentData::Assembly(Default::default()));
        let sig = crate::gui::files::signature_of(&p);
        let other = Project::new("Other");
        let mut tabs = vec![
            Some(tab(p, asm, 1, true, true)),
            Some(tab(Project::new("Untitled"), part, 1, false, false)),
            Some(tab(other, DocumentId::new_random(), 2, true, false)),
            None,
        ];
        assert!(group_dirty(&tabs, 1));
        assert!(!group_dirty(&tabs, 2));
        take_holding(&mut tabs, 1);
        let (a, b) = (tabs[0].as_ref().unwrap(), tabs[1].as_ref().unwrap());
        assert!(!a.holds && b.holds);
        assert!(!a.dirty && b.dirty);
        assert_eq!(crate::gui::files::signature_of(&b.project), sig);
        assert!(b.project.document(part).is_some());
        assert_eq!(holder_of(&tabs, 1), Some(1));
        // Taking it again changes nothing.
        take_holding(&mut tabs, 1);
        assert_eq!(holder_of(&tabs, 1), Some(1));
        assert!(group_dirty(&tabs, 1));
    }

    /// Close Other keeps a changed file whole (all its tabs) and closes the
    /// kept tab's own other tabs and every unchanged file.
    #[test]
    fn close_other_keeps_changed_files_whole() {
        let id = DocumentId::new_random();
        let tabs = vec![
            None, // active, group 1
            Some(tab(Project::new("x"), id, 1, false, false)),
            Some(tab(Project::new("y"), id, 2, true, true)),
            Some(tab(Project::new("y"), id, 2, false, false)),
            Some(tab(Project::new("z"), id, 3, true, false)),
            Some(tab(Project::new("z"), id, 3, false, false)),
        ];
        assert_eq!(
            close_other_set(&tabs, 0, 1),
            vec![false, true, false, false, true, true]
        );
    }
}
