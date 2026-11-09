//! Where the Frame commands work (`docs/spec/6.1-frame-generator.md` §3.6a):
//! the frame assembly placed in the active assembly, or the active assembly
//! itself (a frame assembly opened on its own, a frame saved flat). Member ids
//! in the Frame commands are always ids of that document; the helpers here
//! translate selection, picks and references to and from the active assembly.

use super::CadCraftApp;
use super::asm_planned::AsmPlanned;
use super::asm_select::AsmSub;
use super::part_planned::Planned;
use crate::assembly::frame::{Skeleton, Treatment};
use crate::assembly::frame_sub::{self, Target};
use crate::assembly::relation::GeomRef;
use crate::assembly::tree::Linked;
use crate::document::iprops::BomStructure;
use crate::document::{AssemblyDocument, DocumentData, DocumentId, Project};
use crate::kernel::{Point3, Transform3};
use eframe::egui::{self, Pos2, Rect};

/// The frame the commands work on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct FrameCtx {
    /// The document holding the frame.
    pub doc: DocumentId,
    /// Its occurrence in the active assembly (`None`: it is the active one).
    pub occ: Option<u32>,
    /// Frame coordinates to the active assembly's.
    pub place: Transform3,
}

/// The Create New Frame dialog, shown by the first Insert Frame's OK.
pub(super) struct FrameNew {
    name: String,
    skeleton: String,
    error: Option<String>,
    focus: bool,
}

impl CadCraftApp {
    /// The frame of the active assembly. A frame assembly is found through
    /// the skeleton links, so assemblies without one pay nothing per frame.
    pub(super) fn frame_ctx(&self) -> Option<FrameCtx> {
        let a = self.assembly()?;
        if !a.frames.is_empty() {
            return Some(FrameCtx {
                doc: self.part_id,
                occ: None,
                place: Transform3::identity(),
            });
        }
        let occ = a.frame_links.last()?.frame;
        let o = a.find(occ)?;
        let doc = o.component.document;
        (o.component.library.is_none()
            && matches!(
                self.project.document(doc).map(|d| &d.data),
                Some(DocumentData::Assembly(_))
            ))
        .then_some(FrameCtx {
            doc,
            occ: Some(occ),
            place: o.transform,
        })
    }

    /// The document the Frame commands change.
    pub(super) fn frame_doc(&self) -> DocumentId {
        self.frame_ctx().map_or(self.part_id, |c| c.doc)
    }

    /// The assembly the frame's members are occurrences of.
    pub(super) fn frame_asm(&self) -> Option<&AssemblyDocument> {
        match self.project.document(self.frame_doc()).map(|d| &d.data) {
            Some(DocumentData::Assembly(a)) => Some(a),
            _ => None,
        }
    }

    /// The occurrence holding the frame, when it is a subassembly here.
    pub(super) fn frame_occ(&self) -> Option<u32> {
        self.frame_ctx().and_then(|c| c.occ)
    }

    /// Frame coordinates to the active assembly's.
    pub(super) fn frame_place(&self) -> Transform3 {
        self.frame_ctx()
            .map_or_else(Transform3::identity, |c| c.place)
    }

    /// Is `occ`, an id of the *active* assembly, a member of a flat frame?
    pub(super) fn is_top_frame_member(&self, occ: u32) -> bool {
        self.frame_occ().is_none() && self.is_frame_member(occ)
    }

    /// The selected members (ids in the frame's document).
    pub(super) fn frame_selected(&self) -> Vec<u32> {
        match self.frame_occ() {
            None => self.asm.sel.clone(),
            Some(f) => self
                .asm
                .sub
                .iter()
                .filter_map(|s| match s {
                    AsmSub::Part { occurrence, sub } if *occurrence == f && sub.len() == 1 => {
                        Some(sub[0])
                    }
                    _ => None,
                })
                .filter(|&m| self.is_frame_member(m))
                .collect(),
        }
    }

    /// Selects members (ids in the frame's document).
    pub(super) fn frame_set_selected(&mut self, ids: Vec<u32>) {
        match self.frame_occ() {
            None => self.asm.sel = ids,
            Some(f) => {
                self.asm.sel.clear();
                self.asm.sub = ids
                    .into_iter()
                    .map(|m| AsmSub::Part {
                        occurrence: f,
                        sub: vec![m],
                    })
                    .collect();
            }
        }
    }

    /// A browser row of member `id`, for prehighlighting.
    pub(super) fn frame_tree_hover(&mut self, id: u32) {
        match self.frame_occ() {
            None => self.asm.hover = Some(id),
            Some(f) => {
                self.asm.tree_hover = Some(AsmSub::Part {
                    occurrence: f,
                    sub: vec![id],
                });
            }
        }
    }

    /// The member under the screen point and where the eye ray meets it
    /// (active assembly coordinates).
    pub(super) fn frame_member_under(&self, p: Pos2, rect: Rect) -> Option<(u32, Point3)> {
        let Some(f) = self.frame_occ() else {
            return self.asm_pick_pub(p, rect).map(|(h, at)| (h.occurrence, at));
        };
        let (part, _, t) = self.part_hit(p, rect)?;
        let &[member] = part.sub.as_slice() else {
            return None;
        };
        if part.top != f || !self.is_frame_member(member) {
            return None;
        }
        let (o, d) = self.camera.ray(p, rect)?;
        Some((member, o + d.try_normalize(1e-12)? * t))
    }

    /// The member under the screen point as a selection item, when the frame
    /// is a subassembly here.
    pub(super) fn frame_member_sub(&self, p: Pos2, rect: Rect) -> Option<AsmSub> {
        let occurrence = self.frame_occ()?;
        let (member, _) = self.frame_member_under(p, rect)?;
        Some(AsmSub::Part {
            occurrence,
            sub: vec![member],
        })
    }

    /// May `s` light up under the cursor with the open panel? A member only
    /// for a selector of members or components (rule 19).
    pub(super) fn frame_sub_takes(&self, s: &AsmSub) -> bool {
        let is_member =
            matches!(s, AsmSub::Part { occurrence, .. } if Some(*occurrence) == self.frame_occ());
        if !is_member {
            return true;
        }
        let Some(d) = self.part_planned.as_ref() else {
            return true;
        };
        let Some(key) = d.active else {
            return true;
        };
        super::frame_select::member_colour(d, key).is_some()
            || d.flow()
                .current()
                .is_some_and(|c| c.accepts.contains(&crate::command::flow::PickKind::Body))
    }

    /// The panel's text for member `id`.
    pub(super) fn frame_label(&self, id: u32) -> Option<String> {
        self.frame_asm()?
            .find(id)
            .map(|o| format!("{} · component", o.name))
    }

    /// How a reference of this frame is made in the active assembly's terms.
    fn frame_target<'a>(&self, skeleton: &'a str) -> Option<Target<'a>> {
        let c = self.frame_ctx()?;
        Some(Target {
            top: self.part_id,
            frame: c.occ?,
            doc: c.doc,
            skeleton,
        })
    }

    /// The base of a further skeleton reference's name: that of the first.
    fn frame_skeleton_name(&self) -> String {
        self.frame_asm()
            .and_then(|a| {
                a.occurrences
                    .iter()
                    .find(|o| o.bom == Some(BomStructure::Reference) && !o.visible)
            })
            .map_or_else(
                || "Skeleton".to_owned(),
                |o| o.name.split(':').next().unwrap_or(&o.name).to_owned(),
            )
    }

    /// A skeleton line of the panel in the frame document's terms (made on
    /// `trial`).
    pub(super) fn frame_skeleton_down(
        &self,
        trial: &mut Project,
        linked: &Linked,
        s: &Skeleton,
    ) -> Result<Skeleton, String> {
        let name = self.frame_skeleton_name();
        match self.frame_target(&name) {
            Some(t) => frame_sub::skeleton_down(trial, linked, t, s),
            None => Ok(s.clone()),
        }
    }

    /// Treatments of a panel in the frame document's terms.
    pub(super) fn frame_treatments_down(
        &self,
        trial: &mut Project,
        linked: &Linked,
        list: Vec<Treatment>,
    ) -> Result<Vec<Treatment>, String> {
        let name = self.frame_skeleton_name();
        match self.frame_target(&name) {
            Some(t) => frame_sub::treatments_down(trial, linked, t, list),
            None => Ok(list),
        }
    }

    /// A stored skeleton line in the active assembly's terms.
    pub(super) fn frame_skeleton_up(&self, s: &Skeleton) -> Skeleton {
        match (self.frame_occ(), self.assembly()) {
            (Some(f), Some(a)) => frame_sub::skeleton_up(a, f, s),
            _ => s.clone(),
        }
    }

    /// A stored reference in the active assembly's terms.
    pub(super) fn frame_geom_up(&self, g: &GeomRef) -> GeomRef {
        match (self.frame_occ(), self.assembly()) {
            (Some(f), Some(a)) => frame_sub::geom_up(a, f, g),
            _ => g.clone(),
        }
    }

    /// The box of member `id` (active assembly coordinates).
    pub(super) fn frame_member_box(&self, id: u32) -> Option<(Point3, Point3)> {
        if self.frame_occ().is_none() {
            return self.asm_box(id);
        }
        let place = self.frame_place();
        let o = self.frame_asm()?.find(id)?;
        let (lo, hi) = self.asm_occ_mesh(o)?.bbox?;
        let world = crate::assembly::occurrence_box(o, (lo, hi));
        let corners = [
            place * world.0,
            place * world.1,
            place * Point3::new(world.0.x, world.1.y, world.0.z),
            place * Point3::new(world.1.x, world.0.y, world.1.z),
        ];
        let mut a = corners[0];
        let mut b = corners[0];
        for c in &corners {
            for k in 0..3 {
                a[k] = a[k].min(c[k]);
                b[k] = b[k].max(c[k]);
            }
        }
        Some((a, b))
    }

    /// First Insert Frame in this assembly: its OK asks for the names.
    /// `true`: the dialog was opened and the panel stays.
    pub(super) fn frame_ask_names(&mut self) -> bool {
        if self.frame_ctx().is_some() {
            return false;
        }
        let (name, skeleton) = frame_sub::default_names(&self.project);
        self.asm.frame_new = Some(FrameNew {
            name,
            skeleton,
            error: None,
            focus: true,
        });
        true
    }

    /// The Create New Frame dialog (spec 6.1 §3.6a): OK inserts the panel's
    /// members into a new frame assembly; Cancel returns to the panel.
    pub(super) fn frame_new_window(&mut self, ctx: &egui::Context) {
        let open = self
            .part_planned
            .as_ref()
            .is_some_and(|d| d.kind == Planned::Asm(AsmPlanned::InsertFrame));
        if !open {
            self.asm.frame_new = None;
        }
        let Some(mut st) = self.asm.frame_new.take() else {
            return;
        };
        let mut ok = false;
        let mut cancel = false;
        // The Enter that opened the dialog is not its OK.
        let first = st.focus;
        let win = egui::Window::new("Create New Frame")
            .collapsible(false)
            .resizable(false)
            .default_pos(ctx.content_rect().center() - egui::vec2(170.0, 90.0));
        self.layout
            .place("frame_new", ctx.content_rect(), false, win)
            .show(ctx, |ui| {
                const NOT_YET: &str = "Documents live in the design file; Manage › Documents › \
                                       Save to Separate File moves them out";
                egui::Grid::new("frame_new_grid")
                    .num_columns(2)
                    .spacing([10.0, 6.0])
                    .show(ui, |ui| {
                        ui.label("Frame Name");
                        let r =
                            ui.add(egui::TextEdit::singleline(&mut st.name).desired_width(220.0));
                        if st.focus {
                            r.request_focus();
                            st.focus = false;
                        }
                        ui.end_row();
                        ui.label("Frame Location");
                        ui.add_enabled_ui(false, |ui| {
                            ui.label("In this design file");
                        })
                        .response
                        .on_disabled_hover_text(NOT_YET);
                        ui.end_row();
                        ui.label("Skeleton Name");
                        ui.add(egui::TextEdit::singleline(&mut st.skeleton).desired_width(220.0));
                        ui.end_row();
                        ui.label("Skeleton Location");
                        ui.add_enabled_ui(false, |ui| {
                            ui.label("In this design file");
                        })
                        .response
                        .on_disabled_hover_text(NOT_YET);
                        ui.end_row();
                    });
                if let Some(e) = &st.error {
                    ui.colored_label(super::theme::tokens().error, e);
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ok = ui.add(super::theme::ok_button("OK")).clicked()
                        || (!first && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                    cancel = ui.button("Cancel").clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Escape));
                });
            });
        if cancel {
            return;
        }
        if ok {
            let Some(d) = self.part_planned.clone() else {
                return;
            };
            match self.frame_insert(&d, Some((st.name.trim(), st.skeleton.trim()))) {
                Ok(()) => self.part_planned = None,
                Err(e) => {
                    st.error = Some(e);
                    self.asm.frame_new = Some(st);
                }
            }
            return;
        }
        self.asm.frame_new = Some(st);
    }

    /// Deletes a frame assembly left without members, on `trial`.
    pub(super) fn frame_prune(&self, trial: &mut Project) -> Result<(), String> {
        frame_sub::prune(trial, self.part_id)
    }
}
