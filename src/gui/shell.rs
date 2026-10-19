//! The Shell command (`docs/spec/2.6-shell.md`): pick the faces to remove
//! in the view (a second click removes one), set the thickness and the
//! direction, see the result as a preview, OK / Apply.

use super::CadCraftApp;
use crate::kernel::{FaceId, Point3};
use crate::part::{Feature, ShellFeature};

/// The open Shell command.
#[derive(Clone, Debug)]
pub(super) struct ShellDraft {
    /// The feature being defined (id and name are set on OK).
    pub def: ShellFeature,
    /// Edit Feature of this feature index (OK replaces it in place).
    pub edit: Option<usize>,
}

impl ShellDraft {
    /// Inventor's defaults: Inside, Automatic Face Chain, 2 mm.
    pub(super) fn new() -> Self {
        Self {
            def: ShellFeature::new(2.0),
            edit: None,
        }
    }

    pub(super) fn editing(def: ShellFeature, k: usize) -> Self {
        Self { def, edit: Some(k) }
    }
}

impl CadCraftApp {
    /// Starts Shell; faces selected beforehand become the faces to remove.
    pub(super) fn start_shell(&mut self) {
        self.finish_sketch();
        self.mode = super::Mode::Normal;
        self.cancel_extrude();
        self.work_draft = None;
        self.blend_draft = None;
        let mut d = ShellDraft::new();
        // The preset first: its Automatic Face Chain applies to the picks.
        self.preset_start("Shell", &mut d.def);
        let body = self.part().body.clone();
        for x in std::mem::take(&mut self.sel) {
            if let super::select::SelItem::Face(name) = x
                && let Some(f) = body.find_face(&name)
            {
                d.def.toggle(&body, f, None);
            }
        }
        self.face_sel = None;
        self.edge_sel = None;
        self.shell_draft = Some(d);
    }

    /// A click on face `f` at `at` (model point) while Shell is open.
    pub(super) fn shell_pick(&mut self, f: FaceId, at: Point3) {
        let body = self.part().body.clone();
        if let Some(d) = self.shell_draft.as_mut() {
            d.def.toggle(&body, f, Some(at));
        }
    }

    /// The faces to remove as they are on the current body (for the
    /// highlight): each reference's piece nearest its hint.
    pub(super) fn shell_faces(&self) -> Vec<FaceId> {
        let Some(d) = &self.shell_draft else {
            return Vec::new();
        };
        let body = &self.part().body;
        d.def
            .remove
            .iter()
            .filter_map(|r| {
                let hint = Point3::from(r.hint);
                body.faces()
                    .filter(|(_, f)| f.name.as_deref() == Some(r.face.as_str()))
                    .map(|(id, f)| {
                        let d = f
                            .mesh
                            .as_ref()
                            .and_then(|m| {
                                m.positions
                                    .iter()
                                    .map(|p| (p - hint).norm())
                                    .min_by(|a, b| a.total_cmp(b))
                            })
                            .unwrap_or(f64::INFINITY);
                        (id, d)
                    })
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(id, _)| id)
            })
            .collect()
    }

    /// The preview: edges of the result's new faces, or why it cannot be
    /// built (computed on a worker thread, cached by the definition).
    pub(super) fn shell_preview(&self) -> Result<Vec<Vec<Point3>>, String> {
        let Some(d) = &self.shell_draft else {
            return Err(String::new());
        };
        let key = format!("shell|{:?}|{:?}", d.edit, d.def);
        let mut trial = self.part().clone();
        let (def, edit) = (d.def.clone(), d.edit);
        self.preview_async(key, move || {
            let built = match edit {
                Some(k) => trial.replace_feature(k, Feature::Shell(def)),
                None => trial.add_shell(def),
            };
            built
                .map(|id| Self::feature_polylines(&trial, &id))
                .map_err(|e| e.to_string())
        })
    }

    /// OK (`keep_open` false) or Apply: adds (or replaces) the feature.
    pub(super) fn commit_shell(&mut self, keep_open: bool) {
        let Some(mut d) = self.shell_draft.clone() else {
            return;
        };
        let cmd = if d.edit.is_some() {
            "Edit Feature"
        } else {
            "Shell"
        };
        self.preset_stamp("Shell", &mut d.def);
        self.checkpoint(cmd);
        let built = match d.edit {
            Some(k) => self
                .part_mut()
                .replace_feature(k, Feature::Shell(d.def.clone())),
            None => self.part_mut().add_shell(d.def.clone()),
        };
        if built.is_err()
            && let Some(k) = d.edit
        {
            // Stay in the edit: rolled back again.
            self.part_mut().rollback = Some(k);
            let _ = self.part_mut().regenerate();
        }
        match built {
            Ok(_) => {
                self.file_status = None;
                self.scene_dirty = true;
                self.preset_used("Shell", &d.def, d.edit.is_none());
                if keep_open && d.edit.is_none() {
                    // Apply: a new shell with the same settings.
                    let mut next = d;
                    next.def.remove.clear();
                    self.shell_draft = Some(next);
                } else {
                    self.shell_draft = None;
                    let k = d.edit.unwrap_or(self.part().last_inserted());
                    self.selection = super::Selection::Feature(k);
                }
            }
            Err(e) => {
                self.history.cancel();
                self.file_status = Some(Err(format!("{cmd}: {e}")));
            }
        }
    }
}
