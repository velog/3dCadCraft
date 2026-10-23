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
use super::icons::Icon;
use super::ribbon::Action;
use super::valuefield::ExprValue;
use crate::assembly::{self as asm, Hit, PickMesh};
use crate::document::{AssemblyDocument, DocRef, DocumentData, DocumentId};
use crate::kernel::{Plane, Point3, Real, Transform3, Vector3};

/// An open assembly command.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum AsmTool {
    /// Place: every click puts the document down as a new occurrence.
    Place(DocumentId),
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
    /// The Place Component window is open.
    pub place_window: bool,
    /// Picking triangles per part document (by its body signature).
    meshes: std::cell::RefCell<HashMap<DocumentId, (String, Arc<PickMesh>)>>,
    /// The (selection, hover) the scene was last built with.
    scene_state: (Vec<u32>, Option<u32>),
    /// The assembly a component was opened from (browser *Return*).
    pub came_from: Option<DocumentId>,
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

    fn assembly_mut(&mut self) -> Option<&mut AssemblyDocument> {
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
    fn asm_mesh(&self, r: DocRef) -> Option<Arc<PickMesh>> {
        let part = asm::part_of(&self.project, r)?;
        let sig = part.body_signature();
        let mut cache = self.asm.meshes.borrow_mut();
        if let Some((k, m)) = cache.get(&r.document)
            && *k == sig
        {
            return Some(m.clone());
        }
        let m = Arc::new(PickMesh::of(part));
        cache.insert(r.document, (sig, m.clone()));
        Some(m)
    }

    /// The occurrence under the screen point and the model point hit.
    fn asm_pick(&self, pos: Pos2, rect: Rect) -> Option<(Hit, Point3)> {
        let a = self.assembly()?;
        let (o, d) = self.camera.ray(pos, rect)?;
        let d = d.try_normalize(1e-12)?;
        let h = asm::pick(a, &o, &d, &|r| self.asm_mesh(r))?;
        Some((h, o + d * h.t))
    }

    /// The box of occurrence `id` in assembly coordinates.
    fn asm_box(&self, id: u32) -> Option<(Point3, Point3)> {
        let o = self.assembly()?.find(id)?;
        let local = self.asm_mesh(o.component)?.bbox?;
        Some(asm::occurrence_box(o, local))
    }

    // ---- documents ---------------------------------------------------------

    /// File → New Assembly: an empty assembly in the open project, made
    /// the active document.
    pub(super) fn new_assembly(&mut self) {
        let n = (1..)
            .map(|k| format!("Assembly{k}"))
            .find(|n| !self.project.documents().iter().any(|d| &d.name == n))
            .unwrap_or_else(|| "Assembly".into());
        self.checkpoint("New Assembly");
        let id = self
            .project
            .add_document(n, DocumentData::Assembly(AssemblyDocument::default()));
        self.asm.came_from = None;
        self.activate_document(id);
        self.file_status = Some(Ok(
            "New assembly: Assemble > Place puts the project's parts in it".into(),
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
        self.asm.place_window = false;
        self.part_id = id;
        if let Some(DocumentData::Part(p)) = self.project.document_mut(id).map(|d| &mut d.data) {
            let _ = p.regenerate();
        }
        self.selection = super::Selection::Body;
        self.editing = None;
        self.tab = if self.is_assembly() {
            super::ribbon::Tab::Assemble
        } else {
            super::ribbon::Tab::Model
        };
        self.after_restore();
        self.scene_dirty = true;
        self.fit_view();
    }

    /// Opens the component of occurrence `id`: its part becomes the active
    /// document; the browser's *Return* comes back.
    fn open_component(&mut self, id: u32) {
        let Some(r) = self
            .assembly()
            .and_then(|a| a.find(id))
            .map(|o| o.component)
        else {
            return;
        };
        if r.library.is_some() {
            self.file_status = Some(Err(
                "Open: a library document opens from its own file".into()
            ));
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
            .filter(|o| o.visible)
            .filter_map(|o| self.asm_box(o.id))
            .reduce(|(a, b), (c, d)| (a.inf(&c), b.sup(&d)))
    }

    // ---- commands ------------------------------------------------------------

    /// Runs an Assemble command.
    pub(super) fn asm_apply(&mut self, action: Action) {
        match action {
            Action::AsmPlace => {
                self.asm.tool = None;
                self.asm.place_window = true;
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
        let all = self
            .assembly()
            .is_some_and(|a| ids.iter().all(|&i| a.find(i).is_some_and(|o| o.visible)));
        self.edit_assembly("Visibility", |a| {
            for &i in ids {
                if let Some(o) = a.find_mut(i) {
                    o.visible = !all;
                }
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
    fn place_at(&mut self, doc: DocumentId, at: Point3, grounded_origin: bool) {
        let Some(base) = self.project.document(doc).map(|d| d.name.clone()) else {
            return;
        };
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
            placed = a.place(DocRef::local(doc), &base, t, ground);
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
        for o in a.occurrences.iter().filter(|o| o.visible) {
            let Some(part) = asm::part_of(&self.project, o.component) else {
                continue;
            };
            let tint = if self.asm.sel.contains(&o.id) {
                Some(SEL_TINT)
            } else if self.asm.hover == Some(o.id) {
                Some(HOVER_TINT)
            } else {
                None
            };
            super::material::add_part_meshes_at(g, part, Some(&o.transform), tint);
        }
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
        let modal = self.asm.place_window;
        // Keys: Esc ends the command (then clears the selection), Delete
        // deletes the selected components.
        if response.hovered() || self.asm.tool.is_some() {
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                if self.asm.tool.is_some() {
                    self.asm.tool = None;
                    self.asm.place_turn = None;
                } else {
                    self.asm.sel.clear();
                }
            }
            if ui.input(|i| i.key_pressed(egui::Key::Delete)) && !ctx.egui_wants_keyboard_input() {
                let ids = self.asm.sel.clone();
                self.delete_occurrences(&ids);
            }
        }
        let hit = match (pointer, on_cube, modal) {
            (Some(p), false, false) if self.asm.drag.is_none() => self.asm_pick(p, rect),
            _ => None,
        };
        if self.asm.drag.is_none() {
            self.asm.hover = hit.map(|(h, _)| h.occurrence);
        }
        let primary_click = response.clicked_by(PointerButton::Primary) && !on_cube && !modal;
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
                    Some((h, _)) => self.asm.sel = vec![h.occurrence],
                    None if !ctrl => self.asm.sel.clear(),
                    None => {}
                },
            }
        }
        if response.double_clicked_by(PointerButton::Primary)
            && self.asm.tool.is_none()
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

        // --- context menu ---
        response.context_menu(|ui| self.asm_context_menu(ui, hit.map(|(h, _)| h.occurrence)));

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
            && let Some(local) = self.asm_mesh(DocRef::local(doc)).and_then(|m| m.bbox)
        {
            let turn = self.asm.place_turn.unwrap_or_else(Transform3::identity);
            let t = Transform3::translation(at.x, at.y, at.z) * turn;
            self.draw_box(&painter, rect, local, &t);
        }
        if let Some((c, r)) = circle {
            let stroke = Stroke::new(1.5, Color32::from_rgb(90, 200, 110));
            painter.circle_stroke(c, r, stroke);
            for (dx, dy) in [(0.0, -r), (0.0, r), (-r, 0.0), (r, 0.0)] {
                painter.circle_filled(c + egui::vec2(dx, dy), 3.5, stroke.color);
            }
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
                let v = q - grab;
                let t = Transform3::translation(v.x, v.y, v.z) * d.start;
                self.edit_assembly("Free Move", |a| {
                    if let Some(o) = a.find_mut(d.id) {
                        o.transform = t;
                    }
                });
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
        if !cube_drag {
            match self.mouse {
                MousePreset::Inventor if left && f4 => self.camera.orbit(delta.x, delta.y),
                MousePreset::Inventor if left && f2 => {
                    self.camera.pan(delta.x, delta.y, rect.height())
                }
                MousePreset::Inventor if left && f3 => self.camera.zoom(-delta.y * 3.0),
                MousePreset::Inventor if middle && shift => self.camera.orbit(delta.x, delta.y),
                MousePreset::Inventor if middle => self.camera.pan(delta.x, delta.y, rect.height()),
                MousePreset::Classic if left && !busy => self.camera.orbit(delta.x, delta.y),
                MousePreset::Classic if right || middle => {
                    self.camera.pan(delta.x, delta.y, rect.height())
                }
                _ => {}
            }
        }
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                self.camera
                    .zoom(if self.invert_wheel { -scroll } else { scroll });
            }
        }
        if response.double_clicked_by(PointerButton::Middle) {
            self.fit_view();
        }
    }

    /// The outline of box `local` moved by `t`.
    fn draw_box(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        local: (Point3, Point3),
        t: &Transform3,
    ) {
        let (a, b) = local;
        let corner = |k: usize| {
            t * Point3::new(
                if k & 1 == 0 { a.x } else { b.x },
                if k & 2 == 0 { a.y } else { b.y },
                if k & 4 == 0 { a.z } else { b.z },
            )
        };
        let stroke = Stroke::new(1.5, Color32::from_rgb(255, 160, 50));
        for (i, j) in [
            (0, 1),
            (2, 3),
            (4, 5),
            (6, 7),
            (0, 2),
            (1, 3),
            (4, 6),
            (5, 7),
            (0, 4),
            (1, 5),
            (2, 6),
            (3, 7),
        ] {
            if let (Some(p), Some(q)) = (
                self.camera.project(&corner(i), rect),
                self.camera.project(&corner(j), rect),
            ) {
                painter.line_segment([p, q], stroke);
            }
        }
    }

    fn asm_cursor_hint(&self) -> Option<String> {
        match self.asm.tool? {
            AsmTool::Place(doc) => {
                let name = self.project.document(doc)?.name.clone();
                let next = self.assembly()?.next_name(&name);
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
            ui.separator();
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
    fn occurrence_menu(&mut self, ui: &mut egui::Ui, ids: &[u32]) {
        let Some(a) = self.assembly() else {
            return;
        };
        let grounded = ids.iter().all(|&i| a.find(i).is_some_and(|o| o.grounded));
        let visible = ids.iter().all(|&i| a.find(i).is_some_and(|o| o.visible));
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
        if ids.len() == 1 && ui.button("Open").clicked() {
            self.open_component(ids[0]);
            ui.close();
        }
        if ui.button("Delete").clicked() {
            self.delete_occurrences(ids);
            ui.close();
        }
    }

    // ---- Place Component window -------------------------------------------------

    /// Place Component: the project's parts; *From File...* copies the
    /// parts of another project in.
    pub(super) fn place_window(&mut self, ctx: &egui::Context) {
        if !self.asm.place_window {
            return;
        }
        let parts = asm::placeable(&self.project);
        let mut open = true;
        let mut chosen = None;
        let w = egui::Window::new("Place Component")
            .open(&mut open)
            .resizable(false)
            .collapsible(false);
        self.layout.place("place_component", ctx.content_rect(), false, w).show(ctx, |ui| {
            if parts.is_empty() {
                ui.label("This project has no parts yet.");
            } else {
                ui.label("Parts of this project (placed live: editing a part updates every occurrence):");
            }
            for (id, name) in &parts {
                ui.horizontal(|ui| {
                    super::panels::tree_icon(ui, Icon::Part);
                    if ui.button(name).clicked() {
                        chosen = Some(*id);
                    }
                });
            }
            ui.separator();
            if ui
                .button("From File...")
                .on_hover_text("Copy the parts of another .3dcadcraft project into this one (Append), then pick one")
                .clicked()
            {
                self.open_dialog(super::files::DialogPurpose::PlaceFrom);
            }
        });
        if let Some(doc) = chosen {
            self.asm.place_window = false;
            self.asm.tool = Some(AsmTool::Place(doc));
        } else if !open {
            self.asm.place_window = false;
        }
    }

    /// *From File...*: every part of the project at `path` copied in.
    pub(super) fn place_from_file(&mut self, path: &std::path::Path) {
        let source = match crate::document::Project::load(path) {
            Ok(p) => p,
            Err(e) => {
                self.file_status = Some(Err(format!("Place: {e}")));
                return;
            }
        };
        let parts: Vec<(String, DocumentData)> = source
            .documents()
            .iter()
            .filter(|d| matches!(d.data, DocumentData::Part(_)))
            .map(|d| (d.name.clone(), d.data.clone()))
            .collect();
        if parts.is_empty() {
            self.file_status = Some(Err(format!("Place: {} has no parts", path.display())));
            return;
        }
        self.checkpoint("Place from File");
        let n = parts.len();
        for (name, mut data) in parts {
            if let DocumentData::Part(p) = &mut data {
                let _ = p.regenerate();
            }
            self.project.add_document(name, data);
        }
        self.file_status = Some(Ok(format!(
            "Place: {n} part(s) copied from {}; pick one in Place Component",
            path.display()
        )));
        self.asm.place_window = true;
    }

    // ---- browser and Properties --------------------------------------------------

    /// The active assembly's tree in the browser.
    pub(super) fn assembly_tree(&mut self, ui: &mut egui::Ui, name: &str) {
        let Some(a) = self.assembly() else {
            return;
        };
        let rows: Vec<(u32, String, bool, bool)> = a
            .occurrences
            .iter()
            .map(|o| (o.id, o.name.clone(), o.grounded, o.visible))
            .collect();
        ui.horizontal(|ui| {
            super::panels::tree_icon(ui, Icon::Assembly);
            ui.label(RichText::new(name).strong());
        });
        let mut menu: Option<Vec<u32>> = None;
        let mut open = None;
        ui.indent("occurrences", |ui| {
            for (id, label, grounded, visible) in rows {
                ui.horizontal(|ui| {
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
                        if ui.input(|i| i.modifiers.command || i.modifiers.shift) {
                            if let Some(k) = self.asm.sel.iter().position(|&i| i == id) {
                                self.asm.sel.remove(k);
                            } else {
                                self.asm.sel.push(id);
                            }
                        } else {
                            self.asm.sel = vec![id];
                        }
                    }
                    if r.double_clicked() {
                        open = Some(id);
                    }
                    if r.hovered() {
                        self.asm.hover = Some(id);
                    }
                    r.context_menu(|ui| {
                        if !self.asm.sel.contains(&id) {
                            self.asm.sel = vec![id];
                        }
                        menu = Some(self.asm.sel.clone());
                        let ids = self.asm.sel.clone();
                        self.occurrence_menu(ui, &ids);
                    });
                });
            }
        });
        let _ = menu;
        if let Some(id) = open {
            self.open_component(id);
        }
    }

    /// Properties of the selected occurrence (or of the assembly).
    pub(super) fn assembly_properties(&mut self, ui: &mut egui::Ui) {
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
        let component = self
            .project
            .document(o.component.document)
            .map_or_else(|| "(missing)".to_owned(), |d| d.name.clone());
        ui.label(RichText::new(&o.name).strong());
        egui::Grid::new("occurrence_props")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("Component");
                ui.label(component);
                ui.end_row();
                let mut g = o.grounded;
                ui.label("Grounded");
                if ui.checkbox(&mut g, "").changed() {
                    self.toggle_ground(&[o.id]);
                }
                ui.end_row();
                let mut v = o.visible;
                ui.label("Visibility");
                if ui.checkbox(&mut v, "").changed() {
                    self.toggle_visible(&[o.id]);
                }
                ui.end_row();
            });
        ui.separator();
        ui.label(RichText::new("Position").strong());
        // Offsets and rotations about the assembly axes (X, then Y, then Z).
        let t = o.transform.translation.vector;
        let (rx, ry, rz) = o.transform.rotation.euler_angles();
        let mut vals = [
            t.x,
            t.y,
            t.z,
            rx.to_degrees(),
            ry.to_degrees(),
            rz.to_degrees(),
        ];
        let before = vals;
        egui::Grid::new("occurrence_pos")
            .num_columns(2)
            .show(ui, |ui| {
                for (k, label) in ["X", "Y", "Z", "Rotation X", "Rotation Y", "Rotation Z"]
                    .into_iter()
                    .enumerate()
                {
                    ui.label(label);
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
        }
    }
}
