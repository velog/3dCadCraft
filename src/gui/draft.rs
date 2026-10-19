//! The Face Draft command (`docs/spec/2.8-draft.md`): pick the pull
//! direction (Fixed Edge) or the fixed plane (Fixed Plane), then the faces
//! (where a face is clicked chooses its fixed edge); set the angle; preview,
//! OK / Apply.

use eframe::egui::{self, Color32, Rect, Stroke};

use super::CadCraftApp;
use crate::kernel::{EdgeId, FaceId, Point3};
use crate::part::{DraftFeature, DraftType, Feature, PlaneRef, PullRef};

/// The selector that takes the next click.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DraftInput {
    /// Pull Direction (Fixed Edge).
    Pull,
    /// Fixed Plane.
    Plane,
    /// Faces.
    Faces,
}

/// The open Face Draft command.
#[derive(Clone, Debug)]
pub(super) struct DraftDraft {
    /// The feature being defined (id and name are set on OK).
    pub def: DraftFeature,
    /// Edit Feature of this feature index.
    pub edit: Option<usize>,
    /// The active selector.
    pub input: DraftInput,
}

impl DraftDraft {
    /// Inventor's defaults: Fixed Edge, 1°, Pull Direction first.
    pub(super) fn new() -> Self {
        Self {
            def: DraftFeature::new(1f64.to_radians()),
            edit: None,
            input: DraftInput::Pull,
        }
    }

    pub(super) fn editing(def: DraftFeature, k: usize) -> Self {
        Self {
            def,
            edit: Some(k),
            input: DraftInput::Faces,
        }
    }

    /// The first selector of the type (Pull Direction or Fixed Plane).
    pub(super) fn first_input(&self) -> DraftInput {
        match self.def.draft_type {
            DraftType::FixedEdge => DraftInput::Pull,
            DraftType::FixedPlane => DraftInput::Plane,
        }
    }

    /// Whether the active selector takes faces with their click point.
    pub(super) fn picks_faces(&self) -> bool {
        self.input == DraftInput::Faces
    }
}

impl CadCraftApp {
    pub(super) fn start_draft(&mut self) {
        self.finish_sketch();
        self.mode = super::Mode::Normal;
        self.cancel_extrude();
        self.work_draft = None;
        self.blend_draft = None;
        self.sel.clear();
        self.face_sel = None;
        self.edge_sel = None;
        let mut d = DraftDraft::new();
        self.preset_start("Face Draft", &mut d.def);
        d.input = d.first_input();
        self.draft_draft = Some(d);
    }

    /// A click while the command is open: on a face (with its model point),
    /// a straight edge, or a plane (origin planes included).
    pub(super) fn draft_pick(
        &mut self,
        face: Option<(FaceId, Point3)>,
        edge: Option<EdgeId>,
        plane: Option<PlaneRef>,
    ) {
        let body = self.part().body.clone();
        let Some(d) = self.draft_draft.as_mut() else {
            return;
        };
        match d.input {
            DraftInput::Faces => {
                if let Some((f, at)) = face {
                    d.def.toggle(&body, f, Some(at));
                }
            }
            DraftInput::Pull => {
                let line = edge
                    .filter(|&e| body.edge_direction(e).is_some())
                    .and_then(|e| body.edge(e).and_then(|x| x.name.clone()));
                let pull = match (line, plane) {
                    (Some(name), _) => Some(PullRef::Line(crate::part::LineRef::Edge(name))),
                    (None, Some(p)) => Some(PullRef::Plane(p)),
                    _ => None,
                };
                if let Some(p) = pull {
                    d.def.pull = Some(p);
                    d.input = DraftInput::Faces;
                }
            }
            DraftInput::Plane => {
                if let Some(p) = plane {
                    d.def.plane = Some(p);
                    d.input = DraftInput::Faces;
                }
            }
        }
    }

    /// The faces to draft as they are on the current body.
    pub(super) fn draft_faces(&self) -> Vec<FaceId> {
        let Some(d) = &self.draft_draft else {
            return Vec::new();
        };
        let body = &self.part().body;
        let mut v: Vec<FaceId> = d
            .def
            .faces
            .iter()
            .filter_map(|r| crate::part::shell::face_near(body, &r.face, Point3::from(r.hint)))
            .collect();
        // A fixed plane on a face shows too.
        if let Some(PlaneRef::Face(n)) = &d.def.plane
            && let Some(f) = body.find_face(n)
        {
            v.push(f);
        }
        v
    }

    pub(super) fn draft_preview(&self) -> Result<Vec<Vec<Point3>>, String> {
        let Some(d) = &self.draft_draft else {
            return Err(String::new());
        };
        let key = format!("draft|{:?}|{:?}", d.edit, d.def);
        let mut trial = self.part().clone();
        let (def, edit) = (d.def.clone(), d.edit);
        self.preview_async(key, move || {
            let built = match edit {
                Some(k) => trial.replace_feature(k, Feature::Draft(def)),
                None => trial.add_draft(def),
            };
            built
                .map(|id| Self::feature_polylines(&trial, &id))
                .map_err(|e| e.to_string())
        })
    }

    /// The pull direction as an arrow from the middle of the body.
    pub(super) fn draw_draft_pull(&self, painter: &egui::Painter, rect: Rect) {
        let Some(d) = &self.draft_draft else { return };
        let part = self.part();
        let Ok(pull) = d.def.pull_direction(&part.body, &part.work) else {
            return;
        };
        let pts: Vec<Point3> = part.body.vertices().map(|(_, v)| v.point).collect();
        if pts.is_empty() {
            return;
        }
        let c = pts
            .iter()
            .fold(Point3::origin(), |a, p| a + p.coords / pts.len() as f64);
        let size = pts.iter().map(|p| (p - c).norm()).fold(1.0, f64::max);
        let (Some(a), Some(b)) = (
            self.camera.project(&c, rect),
            self.camera.project(&(c + pull * size * 0.6), rect),
        ) else {
            return;
        };
        let col = Color32::from_rgb(230, 120, 20);
        painter.arrow(a, b - a, Stroke::new(2.5, col));
    }

    /// OK (`keep_open` false) or Apply.
    pub(super) fn commit_draft(&mut self, keep_open: bool) {
        let Some(mut d) = self.draft_draft.clone() else {
            return;
        };
        let cmd = if d.edit.is_some() {
            "Edit Feature"
        } else {
            "Face Draft"
        };
        self.preset_stamp("Face Draft", &mut d.def);
        self.checkpoint(cmd);
        let built = match d.edit {
            Some(k) => self
                .part_mut()
                .replace_feature(k, Feature::Draft(d.def.clone())),
            None => self.part_mut().add_draft(d.def.clone()),
        };
        if built.is_err()
            && let Some(k) = d.edit
        {
            self.part_mut().rollback = Some(k);
            let _ = self.part_mut().regenerate();
        }
        match built {
            Ok(_) => {
                self.file_status = None;
                self.scene_dirty = true;
                self.preset_used("Face Draft", &d.def, d.edit.is_none());
                if keep_open && d.edit.is_none() {
                    // Apply: the same pull / plane and angle, new faces.
                    let mut next = d;
                    next.def.faces.clear();
                    self.draft_draft = Some(next);
                } else {
                    self.draft_draft = None;
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
