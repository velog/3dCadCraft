//! **Delete Face** (3D Model › Modify, `docs/spec/2.14-delete-face.md`): the
//! panel (Select Individual Faces · Lump or Void, the Faces selector, Heal),
//! the preview of the result's edges, OK / Apply / Cancel and Edit Feature.
//! The geometry is `part::body_op::DeleteFaceOp` + `part/exact.rs::delete_face`.

use eframe::egui::{self, RichText};

use super::CadCraftApp;
use super::command_ui::{RowEvents, selector_row};
use super::icons::Icon;
use super::panels::{plabel, psection, warning_box};
use super::select::SelItem;
use crate::command::flow::{Flow, PickKind, Selector, colors};
use crate::kernel::Point3;
use crate::part::body_op::{DeleteFaceOp, DeleteSelect};
use crate::part::{BodyOp, BodyOpFeature, Feature};

/// The open Delete Face command.
#[derive(Clone, Debug)]
pub(super) struct DeleteDraft {
    pub def: DeleteFaceOp,
    /// Edit Feature of this feature index.
    pub edit: Option<usize>,
}

const FACES: &[PickKind] = &[PickKind::Face, PickKind::PlanarFace];

impl DeleteDraft {
    pub(super) fn new() -> Self {
        Self {
            def: DeleteFaceOp::new(),
            edit: None,
        }
    }

    /// The one selector: the faces (Lump or Void: a face of each shell).
    pub(super) fn flow(&self) -> Flow {
        let prompt = match self.def.select {
            DeleteSelect::Faces => "Select the faces to delete",
            DeleteSelect::LumpOrVoid => {
                "Select a face of each lump or void to delete (Select Other reaches a void)"
            }
        };
        Flow::new(
            vec![Selector {
                key: "faces",
                label: "Faces",
                prompt,
                accepts: FACES,
                single: false,
                required: true,
                count: self.def.faces.len(),
                color: colors::FACES,
            }],
            Some("faces"),
        )
    }
}

/// Face `f` added to the faces of `def`, or with a second pick removed.
/// Pieces of one name are told apart by the face each reference resolves to.
fn take_face(
    body: &crate::kernel::BRep,
    def: &mut DeleteFaceOp,
    f: crate::kernel::FaceId,
    at: Option<Point3>,
) {
    let Some(r) = crate::part::shell::face_ref(body, f, at) else {
        return;
    };
    let resolves = |q: &crate::part::FaceRef| {
        crate::part::shell::face_near(body, &q.face, Point3::from(q.hint))
    };
    match def.faces.iter().position(|q| resolves(q) == Some(f)) {
        Some(i) => {
            def.faces.remove(i);
        }
        None => def.faces.push(r),
    }
}

impl CadCraftApp {
    /// **Delete Face**: faces selected before are its faces (rule 17).
    pub(super) fn start_delete_face(&mut self) {
        let pre = self.sel.clone();
        self.finish_sketch();
        self.mode = super::Mode::Normal;
        self.cancel_extrude();
        self.cancel_thicken();
        self.work_draft = None;
        self.blend_draft = None;
        self.face_sel = None;
        self.edge_sel = None;
        let mut d = DeleteDraft::new();
        for x in pre {
            if let SelItem::Face(n) = &x
                && let Some(f) = self.part().body.find_face(n)
            {
                take_face(&self.part().body, &mut d.def, f, None);
            }
        }
        self.sel.clear();
        self.delete_draft = Some(d);
    }

    /// Edit Feature of Delete Face feature `k`.
    pub(super) fn delete_face_edit(&mut self, k: usize) {
        let Some(Feature::BodyOp(BodyOpFeature {
            op: BodyOp::DeleteFace(def),
            ..
        })) = self.part().features.get(k).cloned()
        else {
            return;
        };
        self.roll_back_to(k);
        self.delete_draft = Some(DeleteDraft { def, edit: Some(k) });
    }

    /// A click in the view while Delete Face is open: the face under the
    /// cursor (Select Other's choice when it cycled to a hidden void).
    pub(super) fn delete_face_click(&mut self, p: egui::Pos2, rect: egui::Rect) {
        let Some(mut d) = self.delete_draft.clone() else {
            return;
        };
        let face = match self.plane_under(p, rect, false) {
            Some(crate::part::PlaneRef::Face(n)) => Some(n),
            _ => None,
        };
        let at = self.camera.ray(p, rect).and_then(|(o, dir)| {
            let dir = dir.try_normalize(1e-12)?;
            let (_, t) = self.picker().pick(&o, &dir)?;
            Some(o + dir * t)
        });
        let item = self
            .sel_item_at(p, rect, None, face.as_deref())
            .or_else(|| face.clone().map(SelItem::Face));
        if let Some(SelItem::Face(n)) = item {
            let body = &self.part().body;
            if let Some(f) = body.find_face(&n) {
                take_face(body, &mut d.def, f, at);
            }
        }
        self.delete_draft = Some(d);
    }

    /// The result's edges (worker thread), or why it fails.
    pub(super) fn delete_face_preview(&mut self) -> Result<Vec<Vec<Point3>>, String> {
        let Some(d) = self.delete_draft.as_ref() else {
            return Err(String::new());
        };
        if d.def.validate().is_err() {
            return Err(String::new());
        }
        let key = format!("delete_face|{:?}|{:?}", d.edit, d.def);
        if let Some(r) = self.preview_ready(&key) {
            return r;
        }
        let mut trial = self.part().clone();
        let (def, edit) = (d.def.clone(), d.edit);
        self.preview_async(key, move || {
            let f = BodyOpFeature::new(BodyOp::DeleteFace(def));
            let built = match edit {
                Some(k) => trial.replace_feature(k, Feature::BodyOp(f)),
                None => trial.add_body_op(f),
            };
            built
                .map(|_| super::combine_split::face_edges(&trial, |_| true))
                .map_err(|e| e.to_string())
        })
    }

    /// OK (`keep_open` false) or Apply.
    pub(super) fn commit_delete_face(&mut self, keep_open: bool) {
        let Some(d) = self.delete_draft.clone() else {
            return;
        };
        let cmd = if d.edit.is_some() {
            "Edit Feature"
        } else {
            "Delete Face"
        };
        self.checkpoint(cmd);
        let f = BodyOpFeature::new(BodyOp::DeleteFace(d.def.clone()));
        let built = match d.edit {
            Some(k) => self.part_mut().replace_feature(k, Feature::BodyOp(f)),
            None => self.part_mut().add_body_op(f),
        };
        if built.is_err()
            && let Some(k) = d.edit
        {
            self.part_mut().rollback = Some(k);
            let _ = self.part_mut().regenerate();
        }
        match built {
            Ok(_) => {
                let k = d.edit.unwrap_or(self.part().last_inserted());
                self.file_status = None;
                self.scene_dirty = true;
                if keep_open && d.edit.is_none() {
                    let mut next = DeleteDraft::new();
                    next.def = DeleteFaceOp {
                        faces: Vec::new(),
                        ..d.def
                    };
                    self.delete_draft = Some(next);
                } else {
                    self.delete_draft = None;
                    self.end_feature_edit();
                    self.selection = super::Selection::Feature(k);
                }
            }
            Err(e) => {
                self.history.cancel();
                self.file_status = Some(Err(format!("{cmd}: {e}")));
            }
        }
    }

    /// Closes the command (Cancel / Esc).
    pub(super) fn cancel_delete_face(&mut self) {
        if self.delete_draft.take().is_some() {
            self.end_feature_edit();
        }
    }

    /// The status-bar prompt.
    pub(super) fn delete_face_prompt(&self) -> Option<String> {
        let d = self.delete_draft.as_ref()?;
        let f = d.flow();
        let p = f.current().map_or("", |s| s.prompt);
        let ok = if d.def.validate().is_ok() {
            ", or OK (Enter)"
        } else {
            ""
        };
        Some(format!("Delete Face: {p}{ok}"))
    }

    /// The panel (Properties).
    pub(super) fn delete_face_panel(&mut self, ui: &mut egui::Ui) {
        let Some(mut d) = self.delete_draft.clone() else {
            return;
        };
        self.panel_header(ui, Icon::DeleteFace, "Delete Face");
        let mut ev = RowEvents::default();
        psection(ui, "delete_face", "Delete Face", |ui| {
            plabel(ui, "Select");
            ui.vertical(|ui| {
                for (v, l, tip) in [
                    (
                        DeleteSelect::Faces,
                        "Individual Faces",
                        "Single faces; Heal may close the gap",
                    ),
                    (
                        DeleteSelect::LumpOrVoid,
                        "Lump or Void",
                        "A whole lump (a separate piece of the body) or a void (a cavity, which fills with material)",
                    ),
                ] {
                    if ui.radio(d.def.select == v, l).on_hover_text(tip).clicked()
                        && d.def.select != v
                    {
                        d.def.select = v;
                        d.def.faces.clear();
                    }
                }
            });
            ui.end_row();
            let flow = d.flow();
            for (i, s) in flow.selectors.iter().enumerate() {
                let text = if s.count > 0 {
                    format!("{} selected", s.count)
                } else {
                    String::new()
                };
                selector_row(ui, s, flow.active == Some(i), &text, &mut ev);
                ui.end_row();
            }
            plabel(ui, "");
            let faces = d.def.select == DeleteSelect::Faces;
            ui.add_enabled(faces, egui::Checkbox::new(&mut d.def.heal, "Heal"))
                .on_hover_text(
                    "Extend the neighbouring faces until they close the gap (planar neighbours)",
                )
                .on_disabled_hover_text("A lump or a void goes whole: nothing to heal");
            ui.end_row();
            if faces && !d.def.heal {
                plabel(ui, "");
                ui.label(
                    RichText::new("Without Heal the solid becomes a surface body")
                        .small()
                        .weak(),
                );
                ui.end_row();
            }
        });
        if ev.clear.is_some() {
            d.def.faces.clear();
        }
        let editing = d.edit.is_some();
        self.delete_draft = Some(d.clone());
        let ready = d.def.validate().is_ok();
        let preview = self.delete_face_preview();
        match &preview {
            Err(e) if e == super::PREVIEW_PENDING => {
                ui.label(e);
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(50));
            }
            Err(e) if ready && !e.is_empty() => warning_box(ui, e),
            Err(_) if !ready => {
                if let Err(e) = d.def.validate() {
                    ui.label(RichText::new(e).weak());
                }
            }
            _ => {}
        }
        let ok =
            ready && (preview.is_ok() || matches!(&preview, Err(e) if e == super::PREVIEW_PENDING));
        ui.horizontal(|ui| {
            if ui
                .add_enabled(ok, egui::Button::new("OK").min_size(egui::vec2(60.0, 22.0)))
                .on_hover_text("Enter, Ctrl+Enter")
                .clicked()
                || super::keys::panel_ok_key(ui, ok)
            {
                self.commit_delete_face(false);
            }
            if !editing
                && ui
                    .add_enabled(
                        ok,
                        egui::Button::new("Apply").min_size(egui::vec2(60.0, 22.0)),
                    )
                    .on_hover_text("Create it and keep the command open")
                    .clicked()
            {
                self.commit_delete_face(true);
            }
            if ui
                .add(egui::Button::new("Cancel").min_size(egui::vec2(60.0, 22.0)))
                .clicked()
            {
                self.cancel_delete_face();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::part::PartDocument;

    #[test]
    fn a_second_pick_removes_a_face() {
        let part = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
        let body = &part.body;
        let mut faces = body.faces().map(|(id, _)| id);
        let (a, b) = (faces.next().unwrap(), faces.next().unwrap());
        let mut def = DeleteFaceOp::new();
        take_face(body, &mut def, a, None);
        take_face(body, &mut def, b, None);
        assert_eq!(def.faces.len(), 2);
        take_face(body, &mut def, a, None);
        assert_eq!(def.faces.len(), 1);
        let kept = crate::part::shell::face_near(
            body,
            &def.faces[0].face,
            Point3::from(def.faces[0].hint),
        );
        assert_eq!(kept, Some(b));
    }
}
