//! The assembly environment, step A1 (`docs/spec/3.1-assemblies.md` §3.1):
//! Place (with a box preview on the cursor, 90° turns and Place Grounded
//! at Origin), selection and prehighlight of occurrences, Free Move (a
//! drag in the view plane), Free Rotate (trackball inside the circle, roll
//! on its rim), Grounded / Visibility / Open / Delete, the browser tree,
//! Properties (position and rotation as expression fields) and the scene
//! (each occurrence drawn with its part's appearances, moved by its
//! transform).
//!
//! The data and the geometry (names, ids, picking in the occurrence's own
//! frame) are GUI-free in [`crate::assembly`]. While an assembly is the
//! active document, `part()` is the empty stand-in, so no part command
//! can touch anything (and `apply` refuses commands not valid in an
//! assembly).

use std::collections::HashMap;
use std::sync::Arc;

use eframe::egui::{self, Color32, PointerButton, Pos2, Rect, RichText, Stroke};

use super::CadCraftApp;
use super::ribbon::Action;
use super::valuefield::ExprValue;
use crate::assembly::{self as asm, Hit, PickMesh};
use crate::document::{AssemblyDocument, DocRef, DocumentData, DocumentId};
use crate::kernel::{Plane, Point3, Real, Transform3, Vector3};

/// An open assembly command.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum AsmTool {
    /// Place: every click puts the document down as a new occurrence
    /// (named after `AsmUi::place_name`).
    Place(DocRef),
    /// Free Move: drag any component (grounded ones too).
    FreeMove,
    /// Free Rotate: the component turned (after the first click).
    FreeRotate(Option<u32>),
}

impl AsmTool {
    fn action(self) -> Action {
        match self {
            AsmTool::Place(_) => Action::AsmPlace,
            AsmTool::FreeMove => Action::AsmFreeMove,
            AsmTool::FreeRotate(_) => Action::AsmFreeRotate,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum DragKind {
    /// Along the view plane through `grab`.
    Move { grab: Point3 },
    /// Free rotation about `pivot` (inside the circle).
    Trackball { pivot: Point3 },
    /// In the screen plane about `pivot` (on the circle's rim).
    Roll { pivot: Point3, center: Pos2 },
}

/// A drag of an occurrence in progress.
#[derive(Clone, Copy, Debug)]
pub(super) struct AsmDrag {
    id: u32,
    kind: DragKind,
    start: Transform3,
    last: Pos2,
    /// Free Rotate's circle radius in points.
    radius: f32,
}

/// The assembly environment's UI state.
#[derive(Default)]
pub(super) struct AsmUi {
    /// Selected occurrences (ids).
    pub sel: Vec<u32>,
    /// The occurrence under the cursor.
    pub hover: Option<u32>,
    /// The open command.
    pub tool: Option<AsmTool>,
    drag: Option<AsmDrag>,
    /// The turn given to the next placed occurrences (right-click → Rotate).
    place_turn: Option<Transform3>,
    /// The Place Component window, while it is open.
    pub place: Option<super::place::PlaceDialog>,
    /// The name the placed occurrences get (`bracket` → `bracket:1`).
    pub place_name: String,
    /// The part files the assembly links (loaded once, reloaded when the
    /// file changes on disk).
    pub libs: super::place::LibCache,
    /// Picking triangles per component (by its body signature).
    meshes: std::cell::RefCell<HashMap<DocRef, (String, Arc<PickMesh>)>>,
    /// The (selection, hover) the scene was last built with.
    scene_state: (Vec<u32>, Option<u32>),
    /// The assembly a component was opened from (browser *Return*).
    pub came_from: Option<DocumentId>,
    /// Copied occurrences (Ctrl+C): component, placement, name.
    clipboard: Vec<(DocRef, Transform3, String)>,
    /// Isolate: only these occurrences are drawn and picked until Undo
    /// Isolate (a view state, not saved, as Inventor's temporary
    /// representation).
    pub isolated: Option<Vec<u32>>,
    /// Find in Browser: the row to open and scroll to (taken by the tree).
    pub find_in_browser: Option<u32>,
    /// Analyze Interference: the running search and its result window.
    pub interference: super::interference::InterferenceUi,
}

/// Selected occurrences: blue; under the cursor: green (as faces in a
/// part).
const SEL_TINT: [f32; 3] = [0.25, 0.5, 0.95];
const HOVER_TINT: [f32; 3] = [0.4, 0.82, 0.45];

impl CadCraftApp {
    /// Whether the active document is an assembly.
    pub(super) fn is_assembly(&self) -> bool {
        self.assembly().is_some()
    }

    /// The active assembly.
    pub(super) fn assembly(&self) -> Option<&AssemblyDocument> {
        match self.project.document(self.part_id).map(|d| &d.data) {
            Some(DocumentData::Assembly(a)) => Some(a),
            _ => None,
        }
    }

    pub(super) fn assembly_mut(&mut self) -> Option<&mut AssemblyDocument> {
        match self.project.document_mut(self.part_id).map(|d| &mut d.data) {
            Some(DocumentData::Assembly(a)) => Some(a),
            _ => None,
        }
    }

    /// Changes the active assembly after an Undo checkpoint `label`
    /// (nothing happens without an assembly).
    fn edit_assembly(&mut self, label: &str, f: impl FnOnce(&mut AssemblyDocument)) {
        if !self.is_assembly() {
            return;
        }
        self.checkpoint(label);
        if let Some(a) = self.assembly_mut() {
            f(a);
        }
        self.scene_dirty = true;
    }

    /// The picking triangles of a component (cached by body signature).
    pub(super) fn asm_mesh(&self, r: DocRef) -> Option<Arc<PickMesh>> {
        let sig = self.with_part(r, |p| p.body_signature())?;
        if let Some((k, m)) = self.asm.meshes.borrow().get(&r)
            && *k == sig
        {
            return Some(m.clone());
        }
        let m = Arc::new(self.with_part(r, PickMesh::of)?);
        self.asm.meshes.borrow_mut().insert(r, (sig, m.clone()));
        Some(m)
    }

    /// The occurrence under the screen point and the model point hit.
    pub(super) fn asm_pick_pub(&self, pos: Pos2, rect: Rect) -> Option<(Hit, Point3)> {
        self.asm_pick(pos, rect)
    }

    fn asm_pick(&self, pos: Pos2, rect: Rect) -> Option<(Hit, Point3)> {
        let a = self.assembly()?;
        let (o, d) = self.camera.ray(pos, rect)?;
        let d = d.try_normalize(1e-12)?;
        let h = asm::pick_among(a, &o, &d, &|r| self.asm_mesh(r), &|o| {
            self.asm_isolated(o.id)
        })?;
        Some((h, o + d * h.t))
    }

    /// Whether Isolate leaves occurrence `id` in the view.
    pub(super) fn asm_isolated(&self, id: u32) -> bool {
        self.asm.isolated.as_ref().is_none_or(|v| v.contains(&id))
    }

    /// Whether occurrence `o` is drawn (visible, not suppressed, not
    /// isolated away).
    pub(super) fn asm_shown(&self, o: &crate::document::Occurrence) -> bool {
        o.shown() && self.asm_isolated(o.id)
    }

    /// The box of occurrence `id` in assembly coordinates.
    fn asm_box(&self, id: u32) -> Option<(Point3, Point3)> {
        let o = self.assembly()?.find(id)?;
        let local = self.asm_mesh(o.component)?.bbox?;
        Some(asm::occurrence_box(o, local))
    }

    // ---- documents ---------------------------------------------------------

    /// File → New Assembly: a new file (tab) holding an empty assembly,
    /// as Inventor's new `.iam`; Place links part files into it.
    pub(super) fn new_assembly(&mut self) {
        self.add_blank_tab();
        let mut project = crate::document::Project::new("Assembly1");
        let id = project.add_document(
            "Assembly1",
            DocumentData::Assembly(AssemblyDocument::default()),
        );
        if let Some(d) = project.document_mut(id) {
            d.properties = crate::document::iprops::IProperties::new_document();
        }
        self.project = project;
        self.part_id = id;
        self.asm = Default::default();
        self.cons = Default::default();
        self.file_path.clear();
        self.history.clear();
        self.editing = None;
        self.selection = super::Selection::Body;
        self.after_restore();
        self.fit_view();
        self.mark_saved();
        self.file_status = Some(Ok(
            "New assembly: Assemble > Place (P) picks a part file to place".into(),
        ));
    }

    /// Makes document `id` of the project the active one (browser click,
    /// Open, Return).
    pub(super) fn activate_document(&mut self, id: DocumentId) {
        if id == self.part_id || self.project.document(id).is_none() {
            return;
        }
        self.leave_document();
        self.asm.tool = None;
        self.asm.drag = None;
        self.asm.sel.clear();
        self.asm.hover = None;
        self.asm.place = None;
        self.part_id = id;
        if let Some(DocumentData::Part(p)) = self.project.document_mut(id).map(|d| &mut d.data) {
            let _ = p.regenerate();
        }
        self.selection = super::Selection::Body;
        self.editing = None;
        self.drw.reset();
        self.tab = if self.is_drawing() {
            super::ribbon::Tab::PlaceViews
        } else if self.is_assembly() {
            super::ribbon::Tab::Assemble
        } else {
            super::ribbon::Tab::Model
        };
        self.after_restore();
        self.asm_refresh_status();
        self.scene_dirty = true;
        self.fit_view();
    }

    /// Opens the component of occurrence `id`: its part becomes the active
    /// document; the browser's *Return* comes back.
    pub(super) fn open_component(&mut self, id: u32) {
        let Some(r) = self
            .assembly()
            .and_then(|a| a.find(id))
            .map(|o| o.component)
        else {
            return;
        };
        // A linked part file opens in a tab of its own (Inventor: Open).
        if let Some(lib) = r.library {
            match self.library_path(lib) {
                Some(path) => self.open_path(&path),
                None => self.file_status = Some(Err("Open: the part file was not found".into())),
            }
            return;
        }
        self.asm.came_from = Some(self.part_id);
        self.activate_document(r.document);
    }

    /// Back to the assembly a component was opened from.
    pub(super) fn return_to_assembly(&mut self) {
        if let Some(a) = self.asm.came_from.take() {
            self.activate_document(a);
        }
    }

    /// The box for Zoom All of the assembly.
    pub(super) fn assembly_box(&self) -> Option<(Point3, Point3)> {
        let a = self.assembly()?;
        a.occurrences
            .iter()
            .filter(|o| self.asm_shown(o))
            .filter_map(|o| self.asm_box(o.id))
            .reduce(|(a, b), (c, d)| (a.inf(&c), b.sup(&d)))
    }

    /// The assembly's keys (Inventor's defaults): P Place, C Constrain,
    /// V Free Move, G Free Rotate, J Joint, M Measure, Ctrl+C / Ctrl+V copy and paste
    /// components.
    pub(super) fn assembly_keys(&mut self, ctx: &egui::Context, letter: bool) {
        let pressed = |k: egui::Key| ctx.input(|i| i.key_pressed(k));
        let cmd = ctx.input(|i| i.modifiers.command);
        if letter && !cmd {
            for (k, a) in [
                (egui::Key::P, Action::AsmPlace),
                (egui::Key::C, Action::AsmConstrain),
                (egui::Key::V, Action::AsmFreeMove),
                (egui::Key::G, Action::AsmFreeRotate),
                (egui::Key::J, Action::AsmJoint),
                (egui::Key::M, Action::App(super::commands::AppCmd::Measure)),
            ] {
                if pressed(k) {
                    self.apply(a);
                }
            }
        }
        let (copy, paste) = ctx.input(|i| {
            let ev = |f: fn(&egui::Event) -> bool| i.events.iter().any(f);
            (
                ev(|e| matches!(e, egui::Event::Copy))
                    || (i.modifiers.command && i.key_pressed(egui::Key::C)),
                ev(|e| matches!(e, egui::Event::Paste(_)))
                    || (i.modifiers.command && i.key_pressed(egui::Key::V)),
            )
        });
        if copy {
            self.copy_occurrences();
            // Something in the system clipboard, so Ctrl+V arrives as a paste.
            ctx.copy_text("3DCadCraft components".into());
        }
        if paste {
            self.paste_occurrences();
        }
    }

    /// Ctrl+C: the selected components.
    pub(super) fn copy_occurrences(&mut self) {
        let Some(a) = self.assembly() else {
            return;
        };
        let list: Vec<(DocRef, Transform3, String)> = self
            .asm
            .sel
            .iter()
            .filter_map(|id| a.find(*id))
            .map(|o| {
                let base = o
                    .name
                    .rsplit_once(':')
                    .filter(|(_, k)| k.parse::<u32>().is_ok())
                    .map_or(o.name.as_str(), |(b, _)| b)
                    .to_owned();
                (o.component, o.transform, base)
            })
            .collect();
        if list.is_empty() {
            return;
        }
        self.file_status = Some(Ok(format!(
            "Copied {} component(s): Ctrl+V pastes",
            list.len()
        )));
        self.asm.clipboard = list;
    }

    /// Ctrl+V: one copied component hangs on the cursor as in Place (same
    /// orientation); several are pasted at once beside the originals.
    pub(super) fn paste_occurrences(&mut self) {
        let list = self.asm.clipboard.clone();
        match list.as_slice() {
            [] => {}
            [(r, t, base)] => {
                self.asm.place = None;
                self.cons.draft = None;
                self.asm.place_name = base.clone();
                self.asm.place_turn = Some(Transform3::from_parts(
                    nalgebra::Translation3::identity(),
                    t.rotation,
                ));
                self.asm.tool = Some(AsmTool::Place(*r));
            }
            many => {
                // Beside the copied ones: shifted by their width along X.
                let boxes: Vec<(Point3, Point3)> = many
                    .iter()
                    .filter_map(|(r, t, _)| {
                        let local = self.asm_mesh(*r)?.bbox?;
                        let o = crate::document::Occurrence::new("", *r, *t);
                        Some(asm::occurrence_box(&o, local))
                    })
                    .collect();
                let dx = boxes
                    .iter()
                    .copied()
                    .reduce(|(a, b), (c, d)| (a.inf(&c), b.sup(&d)))
                    .map_or(10.0, |(lo, hi)| (hi.x - lo.x) * 1.2 + 5.0);
                let mut new = Vec::new();
                self.edit_assembly("Paste", |a| {
                    for (r, t, base) in &many.to_vec() {
                        let t = Transform3::translation(dx, 0.0, 0.0) * t;
                        new.push(a.place(*r, base, t, false));
                    }
                });
                self.asm.sel = new;
            }
        }
    }

    // ---- commands ------------------------------------------------------------

    /// Runs an Assemble command.
    pub(super) fn asm_apply(&mut self, action: Action) {
        match action {
            Action::AsmPlace => {
                self.asm.tool = None;
                self.open_place_dialog();
            }
            Action::AsmFreeMove => {
                self.asm.tool =
                    (self.asm.tool != Some(AsmTool::FreeMove)).then_some(AsmTool::FreeMove);
            }
            Action::AsmFreeRotate => {
                let target = (self.asm.sel.len() == 1).then(|| self.asm.sel[0]);
                self.asm.tool = match self.asm.tool {
                    Some(AsmTool::FreeRotate(_)) => None,
                    _ => Some(AsmTool::FreeRotate(target)),
                };
            }
            Action::AsmConstrain => self.start_constrain(),
            Action::AsmUpdate => self.update_assembly(),
            Action::AsmJoint => self.start_joint(),
            Action::AsmBom => self.open_bom(),
            Action::AsmGround => {
                let ids = self.asm.sel.clone();
                if ids.is_empty() {
                    self.file_status = Some(Err("Grounded: select components first".into()));
                    return;
                }
                self.toggle_ground(&ids);
            }
            _ => {}
        }
    }

    /// Assemble › Update: every linked part file read again, the
    /// constraints solved (one Undo step when something moved).
    pub(super) fn update_assembly(&mut self) {
        self.asm.libs = Default::default();
        if !self.is_assembly() {
            return;
        }
        self.checkpoint("Update");
        self.asm_solve(None);
        self.scene_dirty = true;
        let sick = self.cons.last.sick.len();
        self.file_status = Some(if sick == 0 {
            Ok("Update: the part files were read again and the constraints solved".into())
        } else {
            Err(format!(
                "Update: {sick} constraint(s) lost their geometry (red in Relationships)"
            ))
        });
    }

    /// The Assemble command that is open (the ribbon shows it pressed).
    pub(super) fn asm_tool_action(&self) -> Option<Action> {
        self.asm.tool.map(AsmTool::action)
    }

    /// Grounds the occurrences `ids`, or frees them when all are grounded.
    fn toggle_ground(&mut self, ids: &[u32]) {
        let all = self
            .assembly()
            .is_some_and(|a| ids.iter().all(|&i| a.find(i).is_some_and(|o| o.grounded)));
        self.edit_assembly("Grounded", |a| {
            for &i in ids {
                if let Some(o) = a.find_mut(i) {
                    o.grounded = !all;
                }
            }
        });
    }

    fn toggle_visible(&mut self, ids: &[u32]) {
        self.toggle_flag(ids, "Visibility", |o| &mut o.visible);
    }

    /// Turns the flag `field` of the occurrences `ids` off when it is on
    /// for all of them, else on for all (one Undo step `label`).
    pub(super) fn toggle_flag(
        &mut self,
        ids: &[u32],
        label: &str,
        field: fn(&mut crate::document::Occurrence) -> &mut bool,
    ) {
        let all = self.assembly().is_some_and(|a| {
            ids.iter()
                .all(|&i| a.find(i).cloned().is_some_and(|mut o| *field(&mut o)))
        });
        self.edit_assembly(label, |a| {
            for &i in ids {
                if let Some(o) = a.find_mut(i) {
                    *field(o) = !all;
                }
            }
        });
        if label == "Suppress" {
            // Suppressed components leave the selection; the others' places
            // follow their relationships again.
            self.asm.sel.retain(|i| !ids.contains(i));
            self.asm_solve(None);
        }
    }

    /// Isolate: only `ids` stay in the view (and are picked) until Undo
    /// Isolate.
    pub(super) fn isolate(&mut self, ids: Option<Vec<u32>>) {
        self.asm.isolated = ids;
        if let Some(keep) = &self.asm.isolated {
            self.asm.sel.retain(|i| keep.contains(i));
        }
        self.asm.hover = None;
        self.scene_dirty = true;
        self.fit_view();
    }

    /// Hide Others: every occurrence but `ids` gets Visibility off.
    fn hide_others(&mut self, ids: &[u32]) {
        self.edit_assembly("Hide Others", |a| {
            for o in &mut a.occurrences {
                o.visible = ids.contains(&o.id);
            }
        });
    }

    /// Show All: Visibility on for every occurrence (Undo Isolate too).
    fn show_all(&mut self) {
        self.asm.isolated = None;
        self.edit_assembly("Show All", |a| {
            for o in &mut a.occurrences {
                o.visible = true;
            }
        });
    }

    /// Deletes the occurrences `ids` (Delete key, context menu).
    pub(super) fn delete_occurrences(&mut self, ids: &[u32]) {
        if ids.is_empty() {
            return;
        }
        self.edit_assembly("Delete", |a| {
            for &i in ids {
                a.remove(i);
            }
        });
        self.asm.sel.retain(|i| !ids.contains(i));
        self.asm.hover = None;
        if matches!(self.asm.tool, Some(AsmTool::FreeRotate(Some(i))) if ids.contains(&i)) {
            self.asm.tool = Some(AsmTool::FreeRotate(None));
        }
    }

    /// Places `doc` at `at` (its origin there, with the current turn).
    fn place_at(&mut self, doc: DocRef, at: Point3, grounded_origin: bool) {
        let base = self.asm.place_name.clone();
        let turn = self.asm.place_turn.unwrap_or_else(Transform3::identity);
        let t = if grounded_origin {
            turn
        } else {
            Transform3::translation(at.x, at.y, at.z) * turn
        };
        let mut placed = 0;
        self.edit_assembly("Place", |a| {
            // The first component of an assembly is grounded (Inventor).
            let ground = grounded_origin || a.occurrences.is_empty();
            placed = a.place(doc, &base, t, ground);
        });
        self.asm.sel = vec![placed];
        if grounded_origin {
            self.asm.tool = None;
        }
    }

    /// Where a click places a component: on the component under the
    /// cursor, else on the assembly's XY plane.
    fn place_point(&self, pos: Pos2, rect: Rect) -> Option<Point3> {
        if let Some((_, p)) = self.asm_pick(pos, rect) {
            return Some(p);
        }
        self.camera.ray_plane(pos, rect, &Plane::xy())
    }

    // ---- view ------------------------------------------------------------------

    /// The scene of the assembly: each visible occurrence in its part's
    /// appearances, selected ones tinted blue, the one under the cursor
    /// green.
    pub(super) fn assembly_scene(&self, g: &mut super::renderer::SceneGeometry) {
        let Some(a) = self.assembly() else {
            return;
        };
        for o in a.occurrences.iter().filter(|o| self.asm_shown(o)) {
            // The Constrain preview moves components before OK.
            let place = self
                .cons
                .preview
                .as_ref()
                .and_then(|p| p.get(&o.id).copied())
                .unwrap_or(o.transform);
            let tint = if self.asm.sel.contains(&o.id) {
                Some(SEL_TINT)
            } else if self.asm.hover == Some(o.id) {
                Some(HOVER_TINT)
            } else {
                None
            };
            // Disabled: faint; Transparent (or showing an interference):
            // see-through.
            let overlap = self
                .asm
                .interference
                .report
                .as_ref()
                .is_some_and(|r| r.found.iter().any(|i| i.a == o.id || i.b == o.id));
            let alpha = if !o.enabled {
                Some(0.15)
            } else if o.transparent || overlap {
                Some(0.35)
            } else {
                None
            };
            self.with_part(o.component, |part| {
                super::material::add_part_meshes_seen(g, part, Some(&place), tint, alpha)
            });
        }
        self.interference_scene(g);
    }

    /// Everything the view does while an assembly is active: picking,
    /// Place, Free Move / Rotate, navigation, the scene and its overlays.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn assembly_viewport(
        &mut self,
        ui: &mut egui::Ui,
        frame: &eframe::Frame,
        rect: Rect,
        response: &egui::Response,
        pointer: Option<Pos2>,
        on_cube: bool,
        cube_drag: bool,
    ) {
        let ctx = ui.ctx().clone();
        let modal = self.asm.place.is_some();
        // A linked part file saved meanwhile: reload it.
        if self.poll_libraries() {
            self.scene_dirty = true;
            self.asm_solve(None);
        }
        let constraining = self.cons.draft.is_some();
        if constraining
            && (response.hovered() || ctx.memory(|m| m.focused().is_none()))
            && ui.input(|i| i.key_pressed(egui::Key::Escape))
        {
            self.cancel_constrain();
        }
        // Keys: Esc ends the command (then clears the selection), Delete
        // deletes the selected components.
        if response.hovered() || self.asm.tool.is_some() {
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                if self.part_planned.is_some() {
                    self.part_planned = None;
                } else if self.asm.tool.is_some() {
                    self.asm.tool = None;
                    self.asm.place_turn = None;
                } else {
                    self.asm.sel.clear();
                }
            }
            if ui.input(|i| i.key_pressed(egui::Key::Delete)) && !ctx.egui_wants_keyboard_input() {
                let ids = self.asm.sel.clone();
                self.delete_occurrences(&ids);
                if ids.is_empty()
                    && let Some(c) = self.cons.selected
                {
                    self.delete_constraint(c);
                }
            }
        }
        let hit = match (pointer, on_cube, modal) {
            (Some(p), false, false) if self.asm.drag.is_none() => self.asm_pick(p, rect),
            _ => None,
        };
        if self.asm.drag.is_none() {
            self.asm.hover = hit.map(|(h, _)| h.occurrence);
        }
        // Measure (no command open) takes the clicks on faces and edges.
        let measuring = self.measure.open && !constraining && self.asm.tool.is_none();
        // A planned command's panel takes the clicks (`gui/asm_planned.rs`).
        let planned = self.part_planned.is_some();
        if planned
            && response.clicked_by(PointerButton::Primary)
            && !on_cube
            && !modal
            && let Some(p) = pointer
        {
            self.asm_planned_click(p, rect);
        }
        let primary_click = response.clicked_by(PointerButton::Primary)
            && !on_cube
            && !modal
            && !constraining
            && !measuring
            && !planned;
        let measure_click =
            response.clicked_by(PointerButton::Primary) && !on_cube && !modal && measuring;
        // Constrain takes the clicks (drawn and handled after the scene).
        let constrain_click =
            response.clicked_by(PointerButton::Primary) && !on_cube && !modal && constraining;
        let ctrl = ui.input(|i| i.modifiers.command || i.modifiers.shift);

        // --- clicks ---
        if primary_click {
            match self.asm.tool {
                Some(AsmTool::Place(doc)) => {
                    if let Some(p) = pointer.and_then(|p| self.place_point(p, rect)) {
                        self.place_at(doc, p, false);
                    }
                }
                Some(AsmTool::FreeRotate(_)) => {
                    if let Some((h, _)) = hit {
                        self.asm.tool = Some(AsmTool::FreeRotate(Some(h.occurrence)));
                        self.asm.sel = vec![h.occurrence];
                    }
                }
                _ => match hit {
                    Some((h, _)) if ctrl => {
                        let id = h.occurrence;
                        if let Some(k) = self.asm.sel.iter().position(|&i| i == id) {
                            self.asm.sel.remove(k);
                        } else {
                            self.asm.sel.push(id);
                        }
                    }
                    Some((h, _)) => {
                        self.asm.sel = vec![h.occurrence];
                        self.cons.selected = None;
                    }
                    None if !ctrl => {
                        self.asm.sel.clear();
                        self.cons.selected = None;
                    }
                    None => {}
                },
            }
        }
        if response.double_clicked_by(PointerButton::Primary)
            && self.asm.tool.is_none()
            && !constraining
            && let Some((h, _)) = hit
        {
            self.open_component(h.occurrence);
            return;
        }

        // --- drags: Free Rotate's circle, moving a component ---
        let circle = self.rotate_circle(rect);
        if response.drag_started_by(PointerButton::Primary)
            && !on_cube
            && !modal
            && !constraining
            && let Some(press) = ui.input(|i| i.pointer.press_origin())
        {
            self.asm.drag = self.start_drag(press, rect, circle);
        }
        let mut busy = self.asm.drag.is_some();
        if let (Some(d), Some(now)) = (self.asm.drag, response.interact_pointer_pos())
            && response.dragged_by(PointerButton::Primary)
        {
            self.continue_drag(d, now, rect);
        }
        if self.asm.drag.is_some() && !response.dragged() {
            self.asm.drag = None;
            self.history.break_coalescing();
            busy = false;
        }

        // --- navigation ---
        self.asm_navigate(ui, response, rect, cube_drag, busy);
        self.draw_orbit_pivot(ui.ctx(), rect);

        // --- context menu (the ViewCube's own when it was clicked) ---
        let cube_menu = self.cube_context;
        response.context_menu(|ui| {
            if cube_menu {
                return;
            }
            if self.part_planned.is_some() && self.command_context_menu(ui) {
                return;
            }
            if self.cons.draft.is_some() {
                if ui.button("OK").clicked() {
                    self.commit_constrain_pub(true);
                    ui.close();
                }
                if ui.button("Apply").clicked() {
                    self.commit_constrain_pub(false);
                    ui.close();
                }
                if ui.button("Cancel").clicked() {
                    self.cancel_constrain();
                    ui.close();
                }
                return;
            }
            self.asm_context_menu(ui, hit.map(|(h, _)| h.occurrence))
        });

        // --- 3D ---
        let state = (self.asm.sel.clone(), self.asm.hover);
        if state != self.asm.scene_state {
            self.asm.scene_state = state;
            self.scene_dirty = true;
        }
        let painter = ui.painter_at(rect);
        self.render_view(ui, frame, rect, &painter);

        // --- overlays ---
        if let (Some(AsmTool::Place(doc)), Some(p)) = (self.asm.tool, pointer)
            && !on_cube
            && !modal
            && let Some(at) = self.place_point(p, rect)
            && let Some(mesh) = self.asm_mesh(doc)
        {
            // The part itself, see-through, where the click would put it.
            let turn = self.asm.place_turn.unwrap_or_else(Transform3::identity);
            let t = Transform3::translation(at.x, at.y, at.z) * turn;
            super::place::draw_ghost(&painter, &self.camera, rect, &mesh, &t);
        }
        if let Some((c, r)) = circle {
            let stroke = Stroke::new(1.5, Color32::from_rgb(90, 200, 110));
            painter.circle_stroke(c, r, stroke);
            for (dx, dy) in [(0.0, -r), (0.0, r), (-r, 0.0), (r, 0.0)] {
                painter.circle_filled(c + egui::vec2(dx, dy), 3.5, stroke.color);
            }
        }
        let on_view = !on_cube && !modal;
        self.constrain_view(&painter, rect, pointer.filter(|_| on_view), constrain_click);
        if measuring || !self.measure.open {
            self.asm_measure_view(&painter, rect, pointer.filter(|_| on_view), measure_click);
        }
        if let Some(g) = self.cons.browser_hover.clone() {
            self.draw_geom(&painter, rect, &g, Color32::from_rgb(110, 220, 120));
        }
        if let (Some(p), Some(text)) = (pointer, self.asm_cursor_hint()) {
            super::command_ui::cursor_hint(&painter, rect, p, &text);
        }
    }

    /// Free Rotate's circle on screen (centre, radius) around its
    /// component.
    fn rotate_circle(&self, rect: Rect) -> Option<(Pos2, f32)> {
        let Some(AsmTool::FreeRotate(Some(id))) = self.asm.tool else {
            return None;
        };
        let (lo, hi) = self.asm_box(id)?;
        let c3 = Point3::from((lo.coords + hi.coords) / 2.0);
        let c = self.camera.project(&c3, rect)?;
        let r = (0..8)
            .filter_map(|k| {
                let q = Point3::new(
                    if k & 1 == 0 { lo.x } else { hi.x },
                    if k & 2 == 0 { lo.y } else { hi.y },
                    if k & 4 == 0 { lo.z } else { hi.z },
                );
                self.camera.project(&q, rect).map(|s| (s - c).length())
            })
            .fold(0.0f32, f32::max);
        Some((c, r.clamp(40.0, rect.height().min(rect.width()) * 0.45)))
    }

    /// The drag that starts at `press`, if any: Free Rotate's circle, a
    /// component under the cursor (Free Move moves grounded ones too; a
    /// plain drag only the free ones). `None` leaves the drag to the
    /// navigation.
    fn start_drag(&self, press: Pos2, rect: Rect, circle: Option<(Pos2, f32)>) -> Option<AsmDrag> {
        let a = self.assembly()?;
        if let (Some(AsmTool::FreeRotate(Some(id))), Some((c, r))) = (self.asm.tool, circle) {
            let o = a.find(id)?;
            let (lo, hi) = self.asm_box(id)?;
            let pivot = Point3::from((lo.coords + hi.coords) / 2.0);
            let d = (press - c).length();
            let kind = if (d - r).abs() <= 8.0 {
                DragKind::Roll { pivot, center: c }
            } else if d < r {
                DragKind::Trackball { pivot }
            } else {
                return None;
            };
            return Some(AsmDrag {
                id,
                kind,
                start: o.transform,
                last: press,
                radius: r,
            });
        }
        if matches!(
            self.asm.tool,
            Some(AsmTool::Place(_) | AsmTool::FreeRotate(_))
        ) {
            return None;
        }
        let (h, grab) = self.asm_pick(press, rect)?;
        let o = a.find(h.occurrence)?;
        let free_move = self.asm.tool == Some(AsmTool::FreeMove);
        if o.grounded && !free_move {
            return None;
        }
        Some(AsmDrag {
            id: o.id,
            kind: DragKind::Move { grab },
            start: o.transform,
            last: press,
            radius: 0.0,
        })
    }

    fn continue_drag(&mut self, d: AsmDrag, now: Pos2, rect: Rect) {
        let (back, up) = self.camera.frame();
        let right = up.cross(&back);
        let delta = now - d.last;
        match d.kind {
            DragKind::Move { grab } => {
                let Ok(plane) = Plane::from_normal(grab, back) else {
                    return;
                };
                let Some(q) = self.camera.ray_plane(now, rect, &plane) else {
                    return;
                };
                // A grounded one (Free Move) is put where it is dragged and
                // the others solved after it.
                let constrained = self.assembly().is_some_and(|a| {
                    a.find(d.id).is_some_and(|o| !o.grounded)
                        && a.constraints.iter().any(|c| !c.suppressed && c.uses(d.id))
                });
                if constrained {
                    // The grabbed point follows the cursor as far as the
                    // constraints let it: the component moves (or turns)
                    // along its free directions, its partners follow.
                    let drag = asm::Drag {
                        occurrence: d.id,
                        local: d.start.inverse() * grab,
                        target: q,
                    };
                    self.checkpoint("Free Move");
                    self.asm_solve(Some(&drag));
                } else {
                    let v = q - grab;
                    let t = Transform3::translation(v.x, v.y, v.z) * d.start;
                    self.edit_assembly("Free Move", |a| {
                        if let Some(o) = a.find_mut(d.id) {
                            o.transform = t;
                        }
                    });
                    self.asm_solve(None);
                }
            }
            DragKind::Trackball { pivot } => {
                if delta.length() < 0.5 {
                    return;
                }
                // The point under the cursor follows the drag: right → about
                // the screen's up, down → about its right.
                let axis = up * delta.x as Real + right * delta.y as Real;
                let angle = delta.length() as Real / d.radius.max(1.0) as Real;
                self.edit_assembly("Free Rotate", |a| {
                    if let Some(o) = a.find_mut(d.id) {
                        asm::rotate_about(o, pivot, axis, angle);
                    }
                });
                self.asm_solve(None);
                if let Some(x) = self.asm.drag.as_mut() {
                    x.last = now;
                }
            }
            DragKind::Roll { pivot, center } => {
                let (a0, a1) = (d.last - center, now - center);
                if a0.length() < 1.0 || a1.length() < 1.0 {
                    return;
                }
                // Screen angles grow clockwise (y down); about `back`
                // (towards the eye) a positive turn is anticlockwise.
                let da = (a1.y.atan2(a1.x) - a0.y.atan2(a0.x)) as Real;
                let da = (da + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU)
                    - std::f64::consts::PI;
                self.edit_assembly("Free Rotate", |a| {
                    if let Some(o) = a.find_mut(d.id) {
                        asm::rotate_about(o, pivot, back, -da);
                    }
                });
                self.asm_solve(None);
                if let Some(x) = self.asm.drag.as_mut() {
                    x.last = now;
                }
            }
        }
    }

    /// Orbit / pan / zoom as in a part (the mouse preset's buttons); a
    /// left drag that moves a component does not orbit.
    fn asm_navigate(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        rect: Rect,
        cube_drag: bool,
        busy: bool,
    ) {
        use super::MousePreset;
        let delta = response.drag_delta();
        let (shift, f2, f3, f4) = ui.input(|i| {
            (
                i.modifiers.shift,
                i.key_down(egui::Key::F2),
                i.key_down(egui::Key::F3),
                i.key_down(egui::Key::F4),
            )
        });
        let left = response.dragged_by(PointerButton::Primary);
        let middle = response.dragged_by(PointerButton::Middle);
        let right = response.dragged_by(PointerButton::Secondary);
        let orbit = !cube_drag
            && match self.mouse {
                MousePreset::Inventor => (left && f4) || (middle && shift),
                MousePreset::Classic => left && !busy,
            };
        if orbit {
            let press = ui.input(|i| i.pointer.press_origin());
            self.nav_orbit(delta, rect, press);
        } else {
            self.nav_orbit_end();
        }
        if !cube_drag {
            match self.mouse {
                MousePreset::Inventor if left && f2 => {
                    self.camera.pan(delta.x, delta.y, rect.height())
                }
                MousePreset::Inventor if left && f3 => self.camera.zoom(-delta.y * 3.0),
                MousePreset::Inventor if middle && !shift => {
                    self.camera.pan(delta.x, delta.y, rect.height())
                }
                MousePreset::Classic if right || middle => {
                    self.camera.pan(delta.x, delta.y, rect.height())
                }
                _ => {}
            }
        }
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                self.nav_zoom(scroll, response.hover_pos(), rect);
            }
        }
        if response.double_clicked_by(PointerButton::Middle) {
            self.fit_view();
        }
    }

    fn asm_cursor_hint(&self) -> Option<String> {
        if self.asm.tool.is_none() && self.cons.draft.is_none() && self.measure.open {
            return Some(match self.measure.asm_picks.len() {
                1 => "Measure: click a second face or edge".into(),
                _ => "Measure: click a face or an edge".into(),
            });
        }
        match self.asm.tool? {
            AsmTool::Place(_) => {
                let next = self.assembly()?.next_name(&self.asm.place_name);
                Some(format!("Place {next}"))
            }
            AsmTool::FreeMove => Some("Free Move".into()),
            AsmTool::FreeRotate(None) => Some("Free Rotate: click a component".into()),
            AsmTool::FreeRotate(Some(_)) => None,
        }
    }

    /// The status-bar prompt of the assembly environment.
    pub(super) fn asm_prompt(&self) -> String {
        match self.asm.tool {
            Some(AsmTool::Place(_)) => "Place: click in the view to place an occurrence (on a component or the XY plane); right-click for OK, Place Grounded at Origin or a 90° turn; Esc ends".into(),
            Some(AsmTool::FreeMove) => "Free Move: drag a component in the view plane (grounded ones too); Esc ends".into(),
            Some(AsmTool::FreeRotate(None)) => "Free Rotate: click the component to turn; Esc ends".into(),
            Some(AsmTool::FreeRotate(Some(_))) => "Free Rotate: drag inside the circle to turn freely, on its rim to turn in the screen plane; click another component to switch; Esc ends".into(),
            None => match self.assembly().map(|a| a.occurrences.len()) {
                Some(0) => "The assembly is empty. Assemble > Place (P) puts a part of the project in it".into(),
                _ => "Click a component to select it (Ctrl adds), drag one that is not grounded to move it; double-click opens its part".into(),
            },
        }
    }

    /// The view's right-click menu in an assembly.
    fn asm_context_menu(&mut self, ui: &mut egui::Ui, under: Option<u32>) {
        if let Some(AsmTool::Place(doc)) = self.asm.tool {
            if ui.button("OK").clicked() {
                self.asm.tool = None;
                self.asm.place_turn = None;
                ui.close();
            }
            if ui.button("Place Grounded at Origin").clicked() {
                self.place_at(doc, Point3::origin(), true);
                ui.close();
            }
            ui.separator();
            for (label, axis) in [
                ("Rotate 90° about X", Vector3::x()),
                ("Rotate 90° about Y", Vector3::y()),
                ("Rotate 90° about Z", Vector3::z()),
            ] {
                if ui.button(label).clicked() {
                    let turn = Transform3::rotation(axis * std::f64::consts::FRAC_PI_2);
                    self.asm.place_turn =
                        Some(turn * self.asm.place_turn.unwrap_or_else(Transform3::identity));
                    ui.close();
                }
            }
            return;
        }
        if self.asm.tool.is_some() {
            if ui.button("OK").clicked() {
                self.asm.tool = None;
                ui.close();
            }
            return;
        }
        // The component under the cursor joins the selection.
        if let Some(id) = under
            && !self.asm.sel.contains(&id)
        {
            self.asm.sel = vec![id];
        }
        let ids = self.asm.sel.clone();
        if !ids.is_empty() {
            self.occurrence_menu(ui, &ids);
            if ui.button("Copy        Ctrl+C").clicked() {
                self.copy_occurrences();
                ui.close();
            }
            ui.separator();
        }
        // With a selection, its menu above has them already.
        if ids.is_empty() {
            if self.asm.isolated.is_some() && ui.button("Undo Isolate").clicked() {
                self.isolate(None);
                ui.close();
            }
            if ui.button("Show All").clicked() {
                self.show_all();
                ui.close();
            }
        }
        if !self.asm.clipboard.is_empty() && ui.button("Paste       Ctrl+V").clicked() {
            self.paste_occurrences();
            ui.close();
        }
        if ui.button("Place...").clicked() {
            self.asm_apply(Action::AsmPlace);
            ui.close();
        }
        if ui.button("Free Move").clicked() {
            self.asm_apply(Action::AsmFreeMove);
            ui.close();
        }
        if ui.button("Free Rotate").clicked() {
            self.asm_apply(Action::AsmFreeRotate);
            ui.close();
        }
        if ui.button("Zoom All").clicked() {
            self.fit_view();
            ui.close();
        }
    }

    /// Grounded / Visibility / Open / Delete for occurrences (view and
    /// browser menus).
    pub(super) fn occurrence_menu(&mut self, ui: &mut egui::Ui, ids: &[u32]) {
        let Some(a) = self.assembly() else {
            return;
        };
        let all = |f: fn(&crate::document::Occurrence) -> bool| {
            ids.iter().all(|&i| a.find(i).is_some_and(f))
        };
        let grounded = all(|o| o.grounded);
        let visible = all(|o| o.visible);
        let transparent = all(|o| o.transparent);
        let suppressed = all(|o| o.suppressed);
        let enabled = all(|o| o.enabled);
        let isolated = self.asm.isolated.is_some();
        // Inventor's first group: relate the component to another one.
        if ui.button("Constrain...").clicked() {
            self.asm_apply(Action::AsmConstrain);
            ui.close();
        }
        if ui.button("Joint...").clicked() {
            self.asm_apply(Action::AsmJoint);
            ui.close();
        }
        if ui.button("Measure").clicked() {
            self.apply(Action::App(super::commands::AppCmd::Measure));
            ui.close();
        }
        ui.separator();
        if ids.len() == 1 && ui.button("Open").clicked() {
            self.open_component(ids[0]);
            ui.close();
        }
        ui.separator();
        let mut g = grounded;
        if ui.checkbox(&mut g, "Grounded").clicked() {
            self.toggle_ground(ids);
            ui.close();
        }
        let mut v = visible;
        if ui.checkbox(&mut v, "Visibility").clicked() {
            self.toggle_visible(ids);
            ui.close();
        }
        if isolated {
            if ui.button("Undo Isolate").clicked() {
                self.isolate(None);
                ui.close();
            }
        } else if ui.button("Isolate").clicked() {
            self.isolate(Some(ids.to_vec()));
            ui.close();
        }
        if ui.button("Hide Others").clicked() {
            self.hide_others(ids);
            ui.close();
        }
        if ui.button("Show All").clicked() {
            self.show_all();
            ui.close();
        }
        let mut t = transparent;
        if ui.checkbox(&mut t, "Transparent").clicked() {
            self.toggle_flag(ids, "Transparent", |o| &mut o.transparent);
            ui.close();
        }
        let mut s = suppressed;
        if ui.checkbox(&mut s, "Suppress").clicked() {
            self.toggle_flag(ids, "Suppress", |o| &mut o.suppressed);
            ui.close();
        }
        let mut e = enabled;
        if ui.checkbox(&mut e, "Enabled").clicked() {
            self.toggle_flag(ids, "Enabled", |o| &mut o.enabled);
            ui.close();
        }
        if ids.len() == 1 && ui.button("Find in Browser").clicked() {
            self.asm.find_in_browser = Some(ids[0]);
            ui.close();
        }
        if ids.len() == 1 && ui.button("iProperties...").clicked() {
            self.open_component_iproperties(ids[0]);
            ui.close();
        }
        ui.separator();
        use super::asm_planned::AsmPlanned as Ap;
        let one = [
            Ap::Replace,
            Ap::Pattern,
            Ap::Mirror,
            Ap::Copy,
            Ap::Interference,
        ];
        for k in one {
            if ui.button(format!("{}...", k.title())).clicked() {
                self.asm.sel = ids.to_vec();
                self.start_asm_planned(k);
                ui.close();
            }
        }
        ui.separator();
        if ui.button("Delete").clicked() {
            self.delete_occurrences(ids);
            ui.close();
        }
    }

    // ---- browser and Properties --------------------------------------------------

    /// Properties of the selected occurrence (or of the assembly).
    pub(super) fn assembly_properties(&mut self, ui: &mut egui::Ui) {
        if self.part_planned.is_some() {
            self.part_planned_panel(ui);
            return;
        }
        if self.cons.draft.is_some() {
            self.constrain_panel(ui);
            return;
        }
        if self.constraint_properties(ui) {
            return;
        }
        let Some(a) = self.assembly() else {
            return;
        };
        let one = match self.asm.sel.as_slice() {
            [id] => a.find(*id).cloned(),
            _ => None,
        };
        let Some(o) = one else {
            ui.label(RichText::new("Assembly").strong());
            ui.label(format!("{} occurrences", a.occurrences.len()));
            if self.asm.sel.len() > 1 {
                ui.label(format!("{} selected", self.asm.sel.len()));
            }
            return;
        };
        let component = self.ref_name(o.component);
        ui.label(RichText::new(&o.name).strong());
        egui::Grid::new("occurrence_props")
            .min_col_width(super::panels::label_width(ui.ctx()))
            .num_columns(2)
            .show(ui, |ui| {
                super::panels::plabel(ui, "Component");
                ui.label(component);
                ui.end_row();
                let mut g = o.grounded;
                super::panels::plabel(ui, "Grounded");
                if ui.checkbox(&mut g, "").changed() {
                    self.toggle_ground(&[o.id]);
                }
                ui.end_row();
                let mut v = o.visible;
                super::panels::plabel(ui, "Visibility");
                if ui.checkbox(&mut v, "").changed() {
                    self.toggle_visible(&[o.id]);
                }
                ui.end_row();
                type Flag = fn(&mut crate::document::Occurrence) -> &mut bool;
                let flags: [(&str, bool, Flag); 3] = [
                    ("Transparent", o.transparent, |o| &mut o.transparent),
                    ("Enabled", o.enabled, |o| &mut o.enabled),
                    ("Suppress", o.suppressed, |o| &mut o.suppressed),
                ];
                for (label, on, field) in flags {
                    let mut b = on;
                    super::panels::plabel(ui, label);
                    if ui.checkbox(&mut b, "").changed() {
                        self.toggle_flag(&[o.id], label, field);
                    }
                    ui.end_row();
                }
                super::panels::plabel(ui, "DOF");
                ui.label(if o.grounded {
                    "0 (grounded)".to_owned()
                } else {
                    let n = self.cons.last.dof.get(&o.id).copied().unwrap_or(6);
                    format!("{n} of 6")
                });
                ui.end_row();
            });
        ui.separator();
        ui.label(RichText::new("Position").strong());
        // Offsets and rotations about the assembly axes (X, then Y, then Z).
        let t = o.transform.translation.vector;
        let (rx, ry, rz) = o.transform.rotation.euler_angles();
        // Round-off below a nanometre / nano-degree shows as 0 (not "-0").
        let clean = |v: f64| if v.abs() < 1e-9 { 0.0 } else { v };
        let mut vals = [
            clean(t.x),
            clean(t.y),
            clean(t.z),
            clean(rx.to_degrees()),
            clean(ry.to_degrees()),
            clean(rz.to_degrees()),
        ];
        let before = vals;
        egui::Grid::new("occurrence_pos")
            .min_col_width(super::panels::label_width(ui.ctx()))
            .num_columns(2)
            .show(ui, |ui| {
                for (k, label) in ["X", "Y", "Z", "Rotation X", "Rotation Y", "Rotation Z"]
                    .into_iter()
                    .enumerate()
                {
                    super::panels::plabel(ui, label);
                    let suffix = if k < 3 { " mm" } else { "°" };
                    ui.add(ExprValue::new(&mut vals[k]).suffix(suffix));
                    ui.end_row();
                }
            });
        if vals != before {
            let r = nalgebra::UnitQuaternion::from_euler_angles(
                vals[3].to_radians(),
                vals[4].to_radians(),
                vals[5].to_radians(),
            );
            let t =
                Transform3::from_parts(nalgebra::Translation3::new(vals[0], vals[1], vals[2]), r);
            self.edit_assembly("Position", |a| {
                if let Some(x) = a.find_mut(o.id) {
                    x.transform = t;
                }
            });
            self.asm_solve(None);
        }
    }
}
