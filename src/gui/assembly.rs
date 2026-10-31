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
use crate::part::PlaneRef;

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
    /// The menu of a frame corner's glyph (frame, treatment, where; `true`
    /// while the click that opened it is still being handled).
    pub corner_menu: Option<(u32, usize, Pos2, bool)>,
    /// The occurrence under the cursor.
    pub hover: Option<u32>,
    /// The open command.
    pub tool: Option<AsmTool>,
    drag: Option<AsmDrag>,
    /// The turn given to the next placed occurrences (right-click → Rotate).
    place_turn: Option<Transform3>,
    /// The Place Component window, while it is open.
    pub place: Option<super::place::PlaceDialog>,
    /// The Place window was opened by Replace's Browse: the chosen part
    /// becomes Replace's new component instead of being placed.
    pub place_for_replace: bool,
    /// The Place window chooses the Derive panel's source part.
    pub place_for_derive: bool,
    /// The Derive panel's chosen source and its name.
    pub derive_source: Option<(DocRef, String)>,
    /// The bodies an edited derived feature leaves out (kept by Edit).
    pub derive_exclude: Vec<String>,
    /// Replace's new component and the name its occurrences get.
    pub replace_with: Option<(DocRef, String)>,
    /// The name the placed occurrences get (`bracket` → `bracket:1`).
    pub place_name: String,
    /// The part files the assembly links (loaded once, reloaded when the
    /// file changes on disk).
    pub libs: super::place::LibCache,
    /// Picking triangles per component (by its body signature).
    meshes: std::cell::RefCell<HashMap<DocRef, (String, Arc<PickMesh>)>>,
    /// Features, faces and edges selected under Feature priority / Select
    /// Faces and Edges (`gui/asm_select.rs`), and the one under the cursor.
    pub sub: Vec<super::asm_select::AsmSub>,
    pub sub_hover: Option<super::asm_select::AsmSub>,
    /// The feature row under the cursor in the browser (prehighlighted).
    pub tree_hover: Option<super::asm_select::AsmSub>,
    /// The browser's search text.
    pub search: String,
    /// The (selection, hover) the scene was last built with.
    scene_state: (Vec<u32>, Option<u32>),
    /// The frame panel's tinted members the scene was last built with.
    scene_frame: Vec<(u32, [u8; 3])>,
    scene_sub: (
        Vec<super::asm_select::AsmSub>,
        Option<super::asm_select::AsmSub>,
    ),
    /// The assembly a component was opened from (browser *Return*).
    pub came_from: Option<DocumentId>,
    /// The features of the part edited in place seen so far (*Features are
    /// initially adaptive*).
    pub in_place_features: Option<(DocumentId, Vec<String>)>,
    /// Edit in place (spec 3.10 §3.2): the part being edited and where it
    /// lies in the assembly it came from (the rest is drawn around it).
    pub in_place: Option<InPlace>,
    /// The cross-part reference a sketch-plane click on another component
    /// makes (taken by `plane_picked` after its Undo checkpoint).
    pub pending_context: Option<crate::part::context::ContextRef>,
    /// The other component's face under the cursor while a sketch plane is
    /// picked (prehighlight): its component, face and placement in the
    /// edited part's coordinates.
    pub context_hover: Option<ContextHover>,
    /// Why the last Update of cross-part references failed (sick sources,
    /// an assembly that does not settle); `None` when it went through.
    pub context_error: Option<String>,
    /// The references whose source changed since their copies were taken
    /// (the Quick Access Update button), and when that was last looked at.
    pub context_stale: (f64, Vec<String>),
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
    /// Pattern Component: the key of the copies drawn as its preview.
    pub pattern_preview: Option<String>,
    /// Where the view's context menu was opened (cursor, view rect): its
    /// Constrain / Joint take the face under it as the first selection.
    /// `None` when the menu came from the browser.
    pub menu_at: Option<(Pos2, Rect)>,
}

/// A part or subassembly edited in place (spec 3.10 §3.2).
#[derive(Clone, Debug)]
pub(super) struct InPlace {
    /// The top assembly it is edited in (the context drawn around it, the
    /// assembly cross-part references are made in).
    pub assembly: DocumentId,
    /// Its occurrence there and the path inside a subassembly.
    pub top: u32,
    pub sub: Vec<u32>,
    /// The edited part or subassembly.
    pub document: DocumentId,
    /// Its placement in the top assembly.
    pub place: Transform3,
    /// A part of a linked file: edited in its file's tab, away from the
    /// assembly.
    pub linked: Option<LinkedEdit>,
    /// The document Return goes back to: the top assembly, or the
    /// subassembly being edited in place that this was opened from.
    pub parent: DocumentId,
    /// That subassembly's own edit, restored on Return.
    pub outer: Option<Box<InPlace>>,
}

/// The path and placement in the top assembly of what sits at `top`/`sub`
/// (placed `local`) inside the subassembly at `o_top`/`o_sub` (placed
/// `o_place` in the top assembly).
fn nest_in(
    o_top: u32,
    o_sub: &[u32],
    o_place: Transform3,
    top: u32,
    sub: &[u32],
    local: Transform3,
) -> (u32, Vec<u32>, Transform3) {
    let mut path = o_sub.to_vec();
    path.push(top);
    path.extend(sub);
    (o_top, path, o_place * local)
}

/// Where a linked part or subassembly edited in place sits: the top
/// assembly, its path there, its placement in the top assembly and in the
/// document it was opened from.
pub(super) struct LinkedAt {
    pub assembly: DocumentId,
    pub top: u32,
    pub sub: Vec<u32>,
    pub place: Transform3,
    pub local: Transform3,
}

/// Edit in place of a linked part file (spec 3.10 §3.2): the part's file is
/// the active tab, so the assembly around it is kept as copies taken at the
/// start.
#[derive(Clone, Debug)]
pub(super) struct LinkedEdit {
    /// The assembly's file and its name (for *Return*).
    pub assembly_path: String,
    pub assembly_name: String,
    /// The edited part's file.
    pub part_path: std::path::PathBuf,
    /// The other parts in the edited part's coordinates.
    pub context: Vec<(std::rc::Rc<crate::part::PartDocument>, Transform3)>,
    /// What each of them is in the assembly (aligned with `context`), for
    /// cross-part references made against them.
    pub leaves: Vec<LinkedLeaf>,
}

/// One other part of a linked part's Edit in place (spec 3.10 §3.3).
#[derive(Clone, Debug)]
pub(super) struct LinkedLeaf {
    /// Its occurrence path in the assembly.
    pub path: Vec<u32>,
    /// Its component in the assembly file's terms (the hover's key).
    pub component: DocRef,
    /// Its occurrence's name.
    pub name: String,
    /// Its triangles for picking (part coordinates).
    pub mesh: Arc<PickMesh>,
}

/// The other components while a part is edited in place: grey, see-through.
const CONTEXT_GREY: [f32; 3] = [0.62, 0.63, 0.66];
const CONTEXT_ALPHA: f32 = 0.22;

/// An adaptation of a part of this design file: its document and result.
pub(super) type AdaptedIn = (DocRef, crate::assembly::adapt::Adapted);

/// The tooltip of the Adaptive switch.
const ADAPTIVE_TIP: &str = "The part's adaptive sketches and features take their free sizes from this \
occurrence's relationships (right-click a sketch or feature of the part: Adaptive). One occurrence of a part \
can be adaptive";

/// The other component's face under the cursor: component, face and its
/// placement in the edited part's coordinates.
pub(super) type ContextHover = (DocRef, crate::kernel::FaceId, Transform3);

impl CadCraftApp {
    /// Forgets the assembly UI state of the document being left (selection,
    /// open command, picking meshes by `DocRef`), but keeps the linked part
    /// files: they are cached by path and file time, and dropping them made
    /// every switch to an assembly tab load and rebuild each linked part
    /// again inside one frame (the pause on changing tabs).
    pub(super) fn reset_asm_ui(&mut self) {
        let libs = std::mem::take(&mut self.asm.libs);
        // Edit in place of a linked part lives across its tab switch.
        let in_place = self.asm.in_place.take().filter(|ip| ip.linked.is_some());
        self.asm = AsmUi {
            libs,
            in_place,
            ..Default::default()
        };
    }

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
    pub(super) fn edit_assembly(&mut self, label: &str, f: impl FnOnce(&mut AssemblyDocument)) {
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
        if crate::assembly::tree::assembly_of(&self.project, &self.linked(), r).is_some() {
            return self.subassembly_mesh(r);
        }
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

    /// A subassembly's parts as one picking mesh in its own coordinates
    /// (spec 3.1a: it is picked and moved as a whole), cached by its
    /// content and its parts' bodies.
    fn subassembly_mesh(&self, r: DocRef) -> Option<Arc<PickMesh>> {
        let linked = self.linked();
        let sub = crate::assembly::tree::assembly_of(&self.project, &linked, r)?;
        let leaves = crate::assembly::tree::leaves(&self.project, &linked, r, sub);
        let mut sig = serde_json::to_string(sub).unwrap_or_default();
        let mut parts = Vec::new();
        for l in &leaves {
            let c = l.component;
            sig.push_str(
                &self
                    .with_part(c, |p| p.body_signature())
                    .unwrap_or_default(),
            );
            if let Some(m) = self.part_mesh(c) {
                parts.push((m, l.world));
            }
        }
        if let Some((k, m)) = self.asm.meshes.borrow().get(&r)
            && *k == sig
        {
            return Some(m.clone());
        }
        let m = Arc::new(PickMesh::merged(parts.iter().map(|(m, t)| (&**m, *t))));
        self.asm.meshes.borrow_mut().insert(r, (sig, m.clone()));
        Some(m)
    }

    /// A part's picking mesh (never a subassembly's: no recursion here).
    fn part_mesh(&self, r: DocRef) -> Option<Arc<PickMesh>> {
        if crate::assembly::tree::assembly_of(&self.project, &self.linked(), r).is_some() {
            return None;
        }
        self.asm_mesh(r)
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
    pub(super) fn asm_box(&self, id: u32) -> Option<(Point3, Point3)> {
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
        self.reset_asm_ui();
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
        self.follow_patterns();
        self.follow_frames();
        // Derived parts follow their sources (spec 3.10 §3.5, §3.6).
        self.make_follow();
        self.derive_follow();
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
        self.open_doc(r);
    }

    /// Opens component document `r` (a part or a subassembly; Return comes
    /// back to the assembly open now).
    pub(super) fn open_doc(&mut self, r: DocRef) {
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

    /// Opens the part of occurrence `id` with its feature `k` selected
    /// (browser, Modeling View).
    pub(super) fn open_component_feature(&mut self, id: u32, k: usize) {
        let local = self
            .assembly()
            .and_then(|a| a.find(id))
            .is_some_and(|o| o.component.library.is_none());
        self.open_component(id);
        if local && k < self.part().features.len() {
            self.selection = super::Selection::Feature(k);
        }
    }

    /// Zooms to the faces feature `fid` made on occurrence `id`.
    pub(super) fn find_feature_in_window(&mut self, id: u32, fid: &str) {
        let Some(o) = self.assembly().and_then(|a| a.find(id)).cloned() else {
            return;
        };
        let Some(mesh) = self.asm_mesh(o.component) else {
            return;
        };
        let faces: Vec<crate::kernel::FaceId> = self
            .with_part(o.component, |p| {
                p.body
                    .faces()
                    .filter(|(_, f)| {
                        f.name
                            .as_deref()
                            .is_some_and(|n| crate::document::feature_of_face(n) == fid)
                    })
                    .map(|(i, _)| i)
                    .collect()
            })
            .unwrap_or_default();
        let mut bb: Option<(Point3, Point3)> = None;
        for (f, tri) in mesh.triangles() {
            if !faces.contains(f) {
                continue;
            }
            for p in tri {
                let w = o.transform * p;
                bb = Some(match bb {
                    Some((lo, hi)) => (lo.inf(&w), hi.sup(&w)),
                    None => (w, w),
                });
            }
        }
        match bb {
            Some((lo, hi)) => self.camera.fit(lo, hi),
            None => {
                self.file_status = Some(Err(
                    "Find in Window: the feature has no faces on this component".into(),
                ))
            }
        }
    }

    /// The camera `view` seen in coordinates moved by `m` (rigid): the same
    /// picture of the same things, so Edit in place and Return do not jump
    /// (Inventor keeps the assembly's view).
    fn keep_view(&mut self, view: super::camera::OrbitCamera, m: Transform3) {
        self.camera = view.moved(&m);
    }

    /// Back to the assembly a component was opened from.
    pub(super) fn return_to_assembly(&mut self) {
        // "Edit: … in place" no longer holds.
        if self
            .file_status
            .as_ref()
            .is_some_and(|s| s.as_ref().is_ok_and(|t| t.starts_with("Edit: ")))
        {
            self.file_status = None;
        }
        let view = self.camera.clone();
        let place = self.in_place_now().map(|ip| ip.place);
        if let Some(le) = self.in_place_now().and_then(|ip| ip.linked.as_ref()) {
            let (back, part) = (le.assembly_path.clone(), le.part_path.clone());
            // The assembly shows the part as edited, saved or not.
            self.asm
                .libs
                .take_open(&part, std::rc::Rc::new(self.project.clone()));
            let outer = self.in_place_now().and_then(|ip| ip.outer.clone());
            let ip_place = place;
            self.asm.in_place = None;
            match self.tab_of_path(std::path::Path::new(&back)) {
                Some(k) => self.switch_tab(k),
                None => self.open_path(std::path::Path::new(&back)),
            }
            match (outer, ip_place) {
                // Back into the subassembly edited in place it was opened from.
                (Some(o), Some(m)) => {
                    self.keep_view(view, o.place.inverse() * m);
                    self.asm.came_from = Some(o.parent);
                    self.asm.in_place = Some(*o);
                }
                (None, Some(m)) => self.keep_view(view, m),
                _ => {}
            }
            self.scene_dirty = true;
            return;
        }
        // Back into a subassembly edited in place: its own edit again.
        if let Some(ip) = self.in_place_now().cloned()
            && let Some(outer) = ip.outer
        {
            self.asm.came_from = Some(ip.parent);
            self.activate_document(ip.parent);
            self.keep_view(view, outer.place.inverse() * ip.place);
            self.asm.came_from = Some(outer.parent);
            self.asm.in_place = Some(*outer);
            self.scene_dirty = true;
            return;
        }
        self.asm.in_place = None;
        if let Some(a) = self.asm.came_from.take() {
            self.activate_document(a);
            if let Some(m) = place {
                self.keep_view(view, m);
            }
            // What was built on other components follows them.
            if self.is_assembly() {
                self.checkpoint("Update");
                if !self.context_follow() {
                    self.history.cancel();
                }
            }
        }
    }

    /// Edit (double-click, right-click › Edit; spec 3.10 §3.2): the part at
    /// `sub` below occurrence `top` becomes the edited document, drawn where
    /// it lies with the rest of the assembly around it; *Return* comes back.
    /// A subassembly or a linked part file opens as with Open (in place for
    /// them comes later).
    pub(super) fn edit_in_place(&mut self, top: u32, sub: Vec<u32>) {
        let asm_id = self.part_id;
        let linked = self.linked();
        let Some(a) = self.assembly() else {
            return;
        };
        let Ok(n) = crate::assembly::tree::nested(&self.project, &linked, a, top, &sub) else {
            return;
        };
        let (r, name) = (n.component, n.name);
        let Some(local) = a.find(top).map(|t| t.transform * n.at) else {
            return;
        };
        // A subassembly edited in place: what is opened inside it is placed
        // in the top assembly through it.
        let outer = self
            .in_place_now()
            .filter(|ip| ip.linked.is_none() && ip.document == asm_id)
            .cloned();
        let (assembly, top, sub, place) = match &outer {
            Some(o) => {
                let (top, sub, place) = nest_in(o.top, &o.sub, o.place, top, &sub, local);
                (o.assembly, top, sub, place)
            }
            None => (asm_id, top, sub, local),
        };
        // A linked part or subassembly: edited in its file's tab, the rest
        // of the top assembly around it as copies.
        if let Some(lib) = r.library {
            let at = LinkedAt {
                assembly,
                top,
                sub,
                place,
                local,
            };
            self.edit_linked_in_place(lib, r.document, at, outer, name);
            return;
        }
        let view = self.camera.clone();
        self.asm.came_from = Some(asm_id);
        self.activate_document(r.document);
        self.keep_view(view, local.inverse());
        self.asm.came_from = Some(asm_id);
        self.asm.in_place = Some(InPlace {
            assembly,
            top,
            sub,
            document: r.document,
            place,
            linked: None,
            parent: asm_id,
            outer: outer.map(Box::new),
        });
        self.scene_dirty = true;
        self.file_status = Some(Ok(format!("Edit: {name} in place (Return goes back)")));
    }

    /// Edit in place of part `doc` of linked file `lib`: copies of the other
    /// parts are taken, then the part's file opens in its tab.
    fn edit_linked_in_place(
        &mut self,
        lib: crate::document::LibraryId,
        doc: DocumentId,
        at: LinkedAt,
        outer: Option<InPlace>,
        name: String,
    ) {
        let LinkedAt {
            assembly,
            top,
            sub,
            place,
            local,
        } = at;
        let Some(part_path) = self.library_path(lib) else {
            self.file_status = Some(Err("Edit: the file was not found".into()));
            return;
        };
        let linked = self.linked();
        let Some(a) =
            crate::assembly::tree::assembly_of(&self.project, &linked, DocRef::local(assembly))
        else {
            return;
        };
        let mut edited = vec![top];
        edited.extend(&sub);
        let inv = place.inverse();
        let mut parts: HashMap<DocRef, std::rc::Rc<crate::part::PartDocument>> = HashMap::new();
        let mut meshes: HashMap<DocRef, Arc<PickMesh>> = HashMap::new();
        let mut context = Vec::new();
        let mut leaves = Vec::new();
        for l in crate::assembly::tree::leaves(&self.project, &linked, DocRef::local(assembly), a) {
            // The edited part, or every part of the edited subassembly.
            if l.path.starts_with(&edited) {
                continue;
            }
            let p = match parts.get(&l.component) {
                Some(p) => p.clone(),
                None => match self.with_part(l.component, |p| std::rc::Rc::new(p.clone())) {
                    Some(p) => {
                        parts.insert(l.component, p.clone());
                        p
                    }
                    None => continue,
                },
            };
            let mesh = meshes
                .entry(l.component)
                .or_insert_with(|| Arc::new(PickMesh::of(&p)))
                .clone();
            leaves.push(LinkedLeaf {
                path: l.path.clone(),
                component: l.component,
                name: l.occurrence.name.clone(),
                mesh,
            });
            context.push((p, inv * l.world));
        }
        let le = LinkedEdit {
            assembly_path: self.file_path.clone(),
            assembly_name: self.document_title(),
            part_path: part_path.clone(),
            context,
            leaves,
        };
        let asm_id = self.part_id;
        let view = self.camera.clone();
        self.open_path(&part_path);
        if self.file_path.trim() != part_path.display().to_string()
            || self.project.document(doc).is_none()
        {
            return;
        }
        if self.part_id != doc {
            self.activate_document(doc);
        }
        self.keep_view(view, local.inverse());
        self.asm.in_place = Some(InPlace {
            assembly,
            top,
            sub,
            document: doc,
            place,
            linked: Some(le),
            parent: asm_id,
            outer: outer.map(Box::new),
        });
        self.scene_dirty = true;
        self.file_status = Some(Ok(format!(
            "Edit: {name} in place, in its file {} (Return goes back)",
            part_path.display()
        )));
    }

    /// The in-place context while it holds: the edited part is the active
    /// document and came from that assembly (a linked part: its file is the
    /// active tab).
    pub(super) fn in_place_now(&self) -> Option<&InPlace> {
        if let Some(ip) = &self.asm.in_place
            && let Some(le) = &ip.linked
        {
            let here = std::path::Path::new(self.file_path.trim()) == le.part_path;
            return (here && ip.document == self.part_id).then_some(ip);
        }
        self.asm
            .in_place
            .as_ref()
            .filter(|ip| ip.document == self.part_id && self.asm.came_from == Some(ip.parent))
    }

    /// A planar face of another component under the cursor while a part of
    /// this design file is edited in place (spec 3.10 §3.3): its distance
    /// along the (unit) eye ray and the cross-part reference a sketch on it
    /// makes.
    pub(super) fn context_face_under(
        &self,
        pos: Pos2,
        rect: Rect,
    ) -> Option<(f64, crate::part::context::ContextRef, ContextHover)> {
        use crate::part::context::{ContextRef, ContextSource};
        let ip = self.in_place_now()?;
        let (o, d) = self.camera.ray(pos, rect)?;
        let d = d.normalize();
        if let Some(le) = &ip.linked {
            return linked_face_under(ip, le, o, d);
        }
        let linked = self.linked();
        let asm = DocRef::local(ip.assembly);
        let a = crate::assembly::tree::assembly_of(&self.project, &linked, asm)?;
        let mut edited = vec![ip.top];
        edited.extend(&ip.sub);
        let inv = ip.place.inverse();
        let mut best: Option<(f64, Vec<u32>, String, Point3, ContextHover)> = None;
        for l in crate::assembly::tree::leaves(&self.project, &linked, asm, a) {
            if l.path == edited || !l.occurrence.pickable() {
                continue;
            }
            // The leaf's frame seen from the edited part (rigid: same t).
            let to_leaf = (inv * l.world).inverse();
            let (lo, ld) = (to_leaf * o, to_leaf * d);
            let Some((f, t)) = self.asm_mesh(l.component).and_then(|m| m.hit(&lo, &ld)) else {
                continue;
            };
            if best.as_ref().is_some_and(|b| b.0 <= t) {
                continue;
            }
            let name = self
                .with_part(l.component, |p| {
                    p.body.face_plane(f)?;
                    p.body.face(f)?.name.clone()
                })
                .flatten();
            if let Some(name) = name {
                let hover = (l.component, f, inv * l.world);
                best = Some((t, l.path.clone(), name, lo + ld * t, hover));
            }
        }
        let (t, path, face, q, hover) = best?;
        // A part that builds on this one already: a loop (never settles).
        if crate::assembly::context::would_loop(&self.project, &linked, ip.assembly, &edited, &path)
        {
            return None;
        }
        let (&top, sub) = path.split_first()?;
        let label = crate::assembly::tree::nested(&self.project, &linked, a, top, sub)
            .map_or_else(|_| "?".to_owned(), |n| n.name);
        let source = ContextSource {
            assembly: ip.assembly,
            target: edited,
            path,
            face,
            edge: None,
            hint: [q.x, q.y, q.z],
            label: format!("{label} face"),
        };
        let geom = crate::assembly::context::current(&self.project, &linked, a, &source).ok()?;
        Some((
            t,
            ContextRef {
                id: String::new(),
                source,
                geom,
            },
            hover,
        ))
    }

    /// An edge of another component under the cursor while a part of this
    /// design file is edited in place (Project Geometry, spec 3.10 §3.3):
    /// the cross-part reference projecting it makes, and the edge's points
    /// in the edited part's coordinates. Edges behind a face (the part's
    /// own or another component's) are skipped; only straight and circular
    /// edges.
    pub(super) fn context_edge_under(
        &self,
        pos: Pos2,
        rect: Rect,
    ) -> Option<(crate::part::context::ContextRef, Vec<Point3>)> {
        use crate::part::context::{ContextRef, ContextSource};
        let ip = self.in_place_now()?;
        let (o, d) = self.camera.ray(pos, rect)?;
        let d = d.try_normalize(1e-12)?;
        if let Some(le) = &ip.linked {
            return self.linked_edge_under(ip, le, pos, rect, o, d);
        }
        let linked = self.linked();
        let asm = DocRef::local(ip.assembly);
        let a = crate::assembly::tree::assembly_of(&self.project, &linked, asm)?;
        let mut edited = vec![ip.top];
        edited.extend(&ip.sub);
        let inv = ip.place.inverse();
        let leaves: Vec<_> = crate::assembly::tree::leaves(&self.project, &linked, asm, a)
            .into_iter()
            .filter(|l| l.path != edited && l.occurrence.pickable())
            .collect();
        // The nearest face along the ray: the part's own or another's.
        let mut front = self.picker().pick(&o, &d).map(|(_, t)| t);
        for l in &leaves {
            let to_leaf = (inv * l.world).inverse();
            if let Some((_, t)) = self
                .asm_mesh(l.component)
                .and_then(|m| m.hit(&(to_leaf * o), &(to_leaf * d)))
                && front.is_none_or(|f| t < f)
            {
                front = Some(t);
            }
        }
        // (screen distance, path, edge ref, points in the part's frame)
        let mut best: Option<(f32, Vec<u32>, crate::part::EdgeRef, Vec<Point3>)> = None;
        for l in &leaves {
            let t = inv * l.world;
            let found = self.with_part(l.component, |p| {
                self.edge_near(p, &t, pos, rect, o, d, front)
            });
            if let Some(Some((dist, r, pts))) = found
                && best.as_ref().is_none_or(|b| dist < b.0)
            {
                best = Some((dist, l.path.clone(), r, pts));
            }
        }
        let (_, path, edge, pts) = best?;
        if crate::assembly::context::would_loop(&self.project, &linked, ip.assembly, &edited, &path)
        {
            return None;
        }
        let (&top, sub) = path.split_first()?;
        let label = crate::assembly::tree::nested(&self.project, &linked, a, top, sub)
            .map_or_else(|_| "?".to_owned(), |n| n.name);
        let source = ContextSource {
            assembly: ip.assembly,
            target: edited,
            path,
            face: edge.faces[0].clone(),
            hint: edge.hint,
            edge: Some(edge),
            label: format!("{label} edge"),
        };
        let geom = crate::assembly::context::current(&self.project, &linked, a, &source).ok()?;
        Some((
            ContextRef {
                id: String::new(),
                source,
                geom,
            },
            pts,
        ))
    }

    /// The sketch plane under the cursor: the edited part's own (faces,
    /// work and origin planes), or a face of another component in front of
    /// them while editing in place — then `PlaneRef::Context("")`, and the
    /// reference waits in `pending_context` for the click.
    pub(super) fn sketch_plane_under(
        &mut self,
        pos: Pos2,
        rect: Rect,
        click: bool,
    ) -> Option<PlaneRef> {
        let own = self.plane_under_pub(pos, rect);
        let ctx = self.context_face_under(pos, rect);
        let own_t = self.camera.ray(pos, rect).and_then(|(o, d)| {
            let d = d.normalize();
            self.face_hit(&o, &d).map(|(_, t)| t)
        });
        self.asm.context_hover = None;
        match ctx {
            Some((t, c, hover)) if own.is_none() || own_t.is_some_and(|o| t < o) => {
                self.asm.context_hover = Some(hover);
                if click {
                    self.asm.pending_context = Some(c);
                }
                Some(PlaneRef::Context(String::new()))
            }
            _ => own,
        }
    }

    /// The prehighlighted face of another component (see
    /// [`Self::sketch_plane_under`]) as triangles in the edited part's
    /// coordinates.
    pub(super) fn context_hover_mesh(&self) -> Option<crate::kernel::TriangleMesh> {
        self.context_mesh(self.asm.context_hover?)
    }

    /// The face of another component `hover` as triangles in the edited
    /// part's coordinates.
    pub(super) fn context_mesh(&self, hover: ContextHover) -> Option<crate::kernel::TriangleMesh> {
        let (comp, f, t) = hover;
        let mut mesh = self
            .with_context_part(comp, |p| {
                p.body
                    .face_mesh(f, &crate::kernel::TessellationOptions::default())
                    .ok()
            })
            .flatten()?;
        for q in &mut mesh.positions {
            *q = t * *q;
        }
        for n in &mut mesh.normals {
            *n = t * *n;
        }
        Some(mesh)
    }

    /// A face of another component in front of the part's own faces under
    /// the cursor, for an extent (Extrude / Revolve *To*, spec 3.10 §3.3).
    pub(super) fn context_target_under(
        &self,
        pos: Pos2,
        rect: Rect,
    ) -> Option<(crate::part::context::ContextRef, ContextHover)> {
        let (t, c, hover) = self.context_face_under(pos, rect)?;
        let own = self.camera.ray(pos, rect).and_then(|(o, d)| {
            let d = d.normalize();
            self.face_hit(&o, &d).map(|(_, t)| t)
        });
        own.is_none_or(|o| t < o).then_some((c, hover))
    }

    /// The edges of the face of another component `hover`, sampled, in the
    /// edited part's coordinates (Project Geometry's highlight).
    pub(super) fn context_face_edges(&self, hover: ContextHover) -> Vec<Vec<Point3>> {
        let (doc, f, t) = hover;
        let opts = crate::kernel::TessellationOptions::default();
        self.with_context_part(doc, |p| {
            crate::part::select::edges_of_face(&p.body, f)
                .into_iter()
                .filter_map(|e| p.body.sample_edge(e, &opts).ok())
                .map(|pts| pts.iter().map(|q| t * q).collect())
                .collect()
        })
        .unwrap_or_default()
    }

    /// Every edge of the face `face` names as a cross-part reference of its
    /// own, in the assembly edited in place (Project Geometry of a whole
    /// face, `assembly::context::face_outline`).
    pub(super) fn context_face_outline(
        &self,
        face: &crate::part::context::ContextSource,
    ) -> Result<Vec<crate::part::context::ContextRef>, String> {
        if let Some((p, m)) = self.linked_context_part(&face.path) {
            return crate::assembly::context::face_outline_of(&p, face, &m);
        }
        let linked = self.linked();
        let a = crate::assembly::tree::assembly_of(
            &self.project,
            &linked,
            DocRef::local(face.assembly),
        )
        .ok_or("the assembly is not open")?;
        crate::assembly::context::face_outline(&self.project, &linked, a, face)
    }

    /// While a linked part is edited in place: the copy of the other part at
    /// occurrence `path` and its placement in the edited part.
    fn linked_context_part(
        &self,
        path: &[u32],
    ) -> Option<(std::rc::Rc<crate::part::PartDocument>, Transform3)> {
        let le = self.in_place_now()?.linked.as_ref()?;
        let k = le.leaves.iter().position(|l| l.path == path)?;
        le.context.get(k).cloned()
    }

    /// [`Self::with_part`] for another component of the edit in place: the
    /// copy a linked part's edit took, else the part itself.
    fn with_context_part<R>(
        &self,
        doc: DocRef,
        f: impl FnOnce(&crate::part::PartDocument) -> R,
    ) -> Option<R> {
        if let Some(le) = self.in_place_now().and_then(|ip| ip.linked.as_ref()) {
            let k = le.leaves.iter().position(|l| l.component == doc)?;
            return le.context.get(k).map(|(p, _)| f(p));
        }
        self.with_part(doc, f)
    }

    /// [`Self::context_edge_under`] while a linked part is edited in its
    /// tab: the other parts are the copies taken at the start.
    fn linked_edge_under(
        &self,
        ip: &InPlace,
        le: &LinkedEdit,
        pos: Pos2,
        rect: Rect,
        o: Point3,
        d: crate::kernel::Vector3,
    ) -> Option<(crate::part::context::ContextRef, Vec<Point3>)> {
        use crate::part::context::{ContextRef, ContextSource};
        let mut front = self.picker().pick(&o, &d).map(|(_, t)| t);
        for (k, leaf) in le.leaves.iter().enumerate() {
            let to_leaf = le.context[k].1.inverse();
            if let Some((_, t)) = leaf.mesh.hit(&(to_leaf * o), &(to_leaf * d))
                && front.is_none_or(|f| t < f)
            {
                front = Some(t);
            }
        }
        let mut best: Option<(f32, usize, crate::part::EdgeRef, Vec<Point3>)> = None;
        for (k, (p, t)) in le.context.iter().enumerate() {
            if let Some((dist, r, pts)) = self.edge_near(p, t, pos, rect, o, d, front)
                && best.as_ref().is_none_or(|b| dist < b.0)
            {
                best = Some((dist, k, r, pts));
            }
        }
        let (_, k, edge, pts) = best?;
        let mut edited = vec![ip.top];
        edited.extend(&ip.sub);
        let source = ContextSource {
            assembly: ip.assembly,
            target: edited,
            path: le.leaves[k].path.clone(),
            face: edge.faces[0].clone(),
            hint: edge.hint,
            edge: Some(edge),
            label: format!("{} edge", le.leaves[k].name),
        };
        let geom = crate::assembly::context::geom_from(&le.context[k].0, &source, &le.context[k].1)
            .ok()?;
        Some((
            ContextRef {
                id: String::new(),
                source,
                geom,
            },
            pts,
        ))
    }

    /// The visible edge of part `p` (placed `t` in the edited part) nearest
    /// `pos` on screen within the pick radius, not behind `front` along the
    /// eye ray `o`, `d`: its screen distance, reference and points.
    #[allow(clippy::too_many_arguments)]
    fn edge_near(
        &self,
        p: &crate::part::PartDocument,
        t: &Transform3,
        pos: Pos2,
        rect: Rect,
        o: Point3,
        d: crate::kernel::Vector3,
        front: Option<f64>,
    ) -> Option<(f32, crate::part::EdgeRef, Vec<Point3>)> {
        let opts = crate::kernel::TessellationOptions::default();
        let mut mine: Option<(f32, crate::part::EdgeRef, Vec<Point3>)> = None;
        for e in p.body.visible_edges() {
            let Ok(pts) = p.body.sample_edge(e, &opts) else {
                continue;
            };
            let pts: Vec<Point3> = pts.iter().map(|q| t * q).collect();
            for w in pts.windows(2) {
                let (Some(sa), Some(sb)) = (
                    self.camera.project(&w[0], rect),
                    self.camera.project(&w[1], rect),
                ) else {
                    continue;
                };
                let ab = sb - sa;
                let k = ((pos - sa).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
                let dist = (sa + ab * k).distance(pos);
                if dist > super::overlay::EDGE_PICK_PX || mine.as_ref().is_some_and(|m| m.0 <= dist)
                {
                    continue;
                }
                let q = super::closest_on_segment(&w[0], &w[1], &o, &d);
                let te = (q - o).dot(&d);
                if front.is_some_and(|f| f < te - 0.02 * te.abs().max(1.0)) {
                    continue;
                }
                if let Some(r) = crate::part::blend::edge_ref(&p.body, e) {
                    mine = Some((dist, r, pts.clone()));
                }
            }
        }
        mine
    }

    /// The loop of another component's face through edge `edge` (an edge
    /// reference), one cross-part reference per edge
    /// (`assembly::context::edge_loop`); `normal` is the sketch's.
    pub(super) fn context_edge_loop(
        &self,
        edge: &crate::part::context::ContextSource,
        normal: crate::kernel::Vector3,
    ) -> Result<Vec<crate::part::context::ContextRef>, String> {
        if let Some((p, m)) = self.linked_context_part(&edge.path) {
            return crate::assembly::context::edge_loop_of(&p, edge, Some(normal), &m);
        }
        let linked = self.linked();
        let a = crate::assembly::tree::assembly_of(
            &self.project,
            &linked,
            DocRef::local(edge.assembly),
        )
        .ok_or("the assembly is not open")?;
        crate::assembly::context::edge_loop(&self.project, &linked, a, edge, Some(normal))
    }

    /// A new sketch `i` on another component's face (Application Options ›
    /// Assembly): its loops projected (*Enable associative edge/loop
    /// projection*), and everything made a copy when *Cross-part geometry
    /// projection* is off. No Undo step (the sketch's).
    pub(super) fn sketch_on_other_component(&mut self, i: usize) {
        let source = match &self.part().sketches[i].placement.plane {
            PlaneRef::Context(id) => self.part().context_ref(id).map(|c| c.source.clone()),
            _ => None,
        };
        if self.in_place_opts.project_loops
            && let Some(face) = source
            && let Ok(edges) = self.context_face_outline(&face)
        {
            let _ = self.part_mut().project_context_edges(i, edges, false);
        }
        if !self.in_place_opts.associative {
            let _ = self.part_mut().copy_sketch_context(i);
        }
    }

    /// *Features are initially adaptive* (Application Options › Assembly):
    /// every feature added while a part is edited in place becomes adaptive.
    /// Called each frame; the ids seen are kept per edit.
    pub(super) fn in_place_features_adaptive(&mut self) {
        let Some(doc) = self.in_place_now().map(|ip| ip.document) else {
            self.asm.in_place_features = None;
            return;
        };
        if self.is_assembly() || self.is_drawing() {
            return;
        }
        let ids: Vec<String> = self
            .part()
            .features
            .iter()
            .map(|f| f.id().to_owned())
            .collect();
        let known = match &self.asm.in_place_features {
            Some((d, k)) if *d == doc => k.clone(),
            _ => {
                self.asm.in_place_features = Some((doc, ids));
                return;
            }
        };
        let new: Vec<String> = ids
            .iter()
            .filter(|id| !known.contains(id))
            .cloned()
            .collect();
        if new.is_empty() {
            return;
        }
        if self.in_place_opts.features_adaptive {
            for id in &new {
                self.part_mut().set_adaptive(id, true);
            }
        }
        self.asm.in_place_features = Some((doc, ids));
    }

    /// `plane` with a pending cross-part reference added to the active part
    /// (after the caller's Undo checkpoint).
    pub(super) fn take_context_plane(&mut self, plane: PlaneRef) -> PlaneRef {
        match (&plane, self.asm.pending_context.take()) {
            (PlaneRef::Context(id), Some(c)) if id.is_empty() => {
                PlaneRef::Context(self.part_mut().add_context(c))
            }
            _ => plane,
        }
    }

    /// Update of the cross-part references made in the active assembly
    /// (spec 3.10 §3.3): changed copies are written, their parts rebuilt
    /// and the constraints solved again, until nothing changes (at most
    /// `SETTLE_ROUNDS`, as `assembly::context::settle`). No Undo step (call
    /// it after the change's checkpoint); `true` when a part changed.
    /// Failures go to `context_error` and the status bar.
    pub(super) fn context_follow(&mut self) -> bool {
        use crate::assembly::context;
        if !self.is_assembly() {
            return false;
        }
        let linked = self.linked();
        let mut changed = false;
        self.asm.context_error = None;
        self.asm.context_stale.0 = f64::NEG_INFINITY;
        for _ in 0..context::SETTLE_ROUNDS {
            let r = context::update(&mut self.project, &linked, self.part_id);
            if !r.sick.is_empty() {
                self.asm.context_error = Some(format!("Update: {}", r.sick.join("; ")));
            }
            if r.changed.is_empty() {
                if let Some(e) = &self.asm.context_error {
                    self.file_status = Some(Err(e.clone()));
                }
                return changed;
            }
            changed = true;
            self.scene_dirty = true;
            // A linked part's copies may have changed (`linked_edits`).
            self.asm.libs.forget_linked();
            self.asm_solve_once(None);
        }
        let still = context::out_of_date(&self.project, &linked, self.part_id);
        let e = format!(
            "Update: the assembly does not settle ({})",
            still.join(", ")
        );
        self.file_status = Some(Err(e.clone()));
        self.asm.context_error = Some(e);
        changed
    }

    /// The Quick Access toolbar's Update button of an assembly: shown while
    /// a cross-part reference is out of date (its source changed, e.g. in
    /// its own tab), checked once a second. Click: Update.
    pub(super) fn context_update_button(&mut self, ui: &mut egui::Ui) {
        if !self.is_assembly() {
            return;
        }
        let now = ui.input(|i| i.time);
        if now - self.asm.context_stale.0 > 1.0 || now < self.asm.context_stale.0 {
            let linked = self.linked();
            self.asm.context_stale = (
                now,
                crate::assembly::context::out_of_date(&self.project, &linked, self.part_id),
            );
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs(1));
        }
        let stale = &self.asm.context_stale.1;
        if stale.is_empty() {
            return;
        }
        let tip = format!(
            "Out of date: {} changed since it was referenced. Update rebuilds the parts built on it",
            stale.join(", ")
        );
        if ui
            .button(
                egui::RichText::new("Update")
                    .color(ui.visuals().warn_fg_color)
                    .strong(),
            )
            .on_hover_text(tip)
            .clicked()
        {
            self.update_assembly();
        }
    }

    /// The other components around a part edited in place: see-through and
    /// grey, in the part's coordinates.
    pub(super) fn in_place_scene(&self, g: &mut super::renderer::SceneGeometry) {
        let Some(ip) = self.in_place_now() else {
            return;
        };
        let grey = |_: Option<&str>| super::material::FaceOver {
            look: None,
            tint: Some(CONTEXT_GREY),
        };
        if let Some(le) = &ip.linked {
            for (p, t) in &le.context {
                super::material::add_part_meshes_with(g, p, Some(t), Some(CONTEXT_ALPHA), &grey);
            }
            return;
        }
        let linked = self.linked();
        let Some(a) =
            crate::assembly::tree::assembly_of(&self.project, &linked, DocRef::local(ip.assembly))
        else {
            return;
        };
        let mut edited = vec![ip.top];
        edited.extend(&ip.sub);
        let inv = ip.place.inverse();
        for l in
            crate::assembly::tree::leaves(&self.project, &linked, DocRef::local(ip.assembly), a)
        {
            // The edited part, or every part of the edited subassembly.
            if l.path.starts_with(&edited) {
                continue;
            }
            let t = inv * l.world;
            self.with_part(l.component, |p| {
                super::material::add_part_meshes_with(g, p, Some(&t), Some(CONTEXT_ALPHA), &grey)
            });
        }
    }

    /// The box for Zoom All of the assembly.
    pub(super) fn assembly_box(&self) -> Option<(Point3, Point3)> {
        let a = self.assembly()?;
        // The components' visible sketch lines too (a frame's skeleton
        // sketch has no body).
        let lines = self
            .sketch_lines()
            .into_iter()
            .map(|(_, _, p, q)| (p.inf(&q), p.sup(&q)));
        a.occurrences
            .iter()
            .filter(|o| self.asm_shown(o))
            .filter_map(|o| self.asm_box(o.id))
            .chain(lines)
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
        if let Some(e) = self.asm.context_error.clone() {
            self.file_status = Some(Err(e));
            return;
        }
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

    pub(super) fn toggle_visible(&mut self, ids: &[u32]) {
        self.toggle_flag(ids, "Visibility", |o| &mut o.visible);
    }

    /// Turns the flag `field` of the occurrences `ids` off when it is on
    /// for all of them, else on for all (one Undo step `label`).
    /// Adaptive on / off for occurrence `id` (spec 3.10 §3.4): on makes the
    /// other occurrences of its part plain (one adaptive occurrence per
    /// part), and the assembly is solved again so the part adapts.
    pub(super) fn toggle_adaptive(&mut self, id: u32) {
        let Some(o) = self.assembly().and_then(|a| a.find(id)).cloned() else {
            return;
        };
        self.edit_assembly("Adaptive", |a| {
            for x in &mut a.occurrences {
                if x.id == id {
                    x.adaptive = !o.adaptive;
                } else if x.component == o.component {
                    x.adaptive = false;
                }
            }
        });
        if !o.adaptive {
            self.asm_solve(None);
        }
    }

    /// Adaptivity on a copy `a` of the active assembly whose rigid solve
    /// does not hold: the first adaptive occurrence of a part of this
    /// design file whose sizes can meet the relationships, with the part's
    /// new state and the solve. `Err`: why none could.
    pub(super) fn adapt_copy(
        &self,
        a: &crate::document::AssemblyDocument,
    ) -> Result<Option<AdaptedIn>, String> {
        let mut why = None;
        for o in a.occurrences.iter().filter(|o| o.adaptive && !o.suppressed) {
            // A part of this file or of a linked part file (only that file's
            // own parts: one linked level).
            if o.component
                .library
                .is_some_and(|l| self.project.library(l).is_none())
            {
                continue;
            }
            let Some(base) = self.with_part(o.component, Clone::clone) else {
                continue;
            };
            let local = |r: &crate::assembly::GeomRef| self.asm_local_in(a, r);
            match crate::assembly::adapt::adapt(a, o.id, &base, &local) {
                Ok(Some(r)) => return Ok(Some((o.component, r))),
                Ok(None) => {}
                Err(e) => why = Some(format!("{}: {e}", o.name)),
            }
        }
        // Adaptive parts inside subassemblies (spec 3.10 §3.4).
        for (target, doc, name) in self.nested_adaptive(a) {
            let Some(DocumentData::Part(base)) = self.project.document(doc).map(|d| &d.data) else {
                continue;
            };
            let local = |r: &crate::assembly::GeomRef| self.asm_local_in(a, r);
            match crate::assembly::adapt::adapt_nested(a, &target, base, &local, None) {
                Ok(Some(r)) => return Ok(Some((DocRef::local(doc), r))),
                Ok(None) => {}
                Err(e) => why = Some(format!("{name}: {e}")),
            }
        }
        why.map_or(Ok(None), Err)
    }

    /// The adaptive parts of this design file inside the subassemblies of
    /// `a`: where they are, their document and name.
    pub(super) fn nested_adaptive(
        &self,
        a: &crate::document::AssemblyDocument,
    ) -> Vec<(crate::assembly::adapt::Target, DocumentId, String)> {
        let linked = self.linked();
        crate::assembly::tree::leaves(&self.project, &linked, DocRef::local(self.part_id), a)
            .into_iter()
            .filter(|l| l.path.len() > 1 && l.occurrence.adaptive && l.component.library.is_none())
            .filter_map(|l| {
                let top = a.find(l.path[0])?;
                let target = crate::assembly::adapt::Target {
                    top: l.path[0],
                    sub: l.path[1..].to_vec(),
                    at: top.transform.inverse() * l.world,
                };
                Some((target, l.component.document, l.occurrence.name.clone()))
            })
            .collect()
    }

    /// Writes an adaptation found by [`Self::adapt_copy`] (after the
    /// caller's checkpoint): the part's new state and the placements.
    pub(super) fn adapt_write(&mut self, a: &mut crate::document::AssemblyDocument, r: AdaptedIn) {
        let (doc, ad) = r;
        match doc.library {
            // A part of a linked file: an edit of that file, saved with this one.
            Some(lib) => self.edit_linked_part(lib, doc.document, ad.part),
            None => {
                if let Some(DocumentData::Part(p)) =
                    self.project.document_mut(doc.document).map(|d| &mut d.data)
                {
                    *p = ad.part;
                }
            }
        }
        for (id, t) in &ad.solved.transforms {
            if let Some(o) = a.find_mut(*id) {
                o.transform = *t;
            }
        }
        self.cons.last = ad.solved;
        self.scene_dirty = true;
    }

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
        // Frame members go with their parts and end treatments.
        let (members, ids): (Vec<u32>, Vec<u32>) =
            ids.iter().partition(|&&o| self.is_frame_member(o));
        if !members.is_empty() {
            self.frame_delete_members(&members);
        }
        let ids = ids.as_slice();
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
        let looks = self.asm_scene_looks();
        for o in a.occurrences.iter().filter(|o| self.asm_shown(o)) {
            // The Constrain preview moves components before OK.
            let place = self
                .cons
                .preview
                .as_ref()
                .and_then(|p| p.get(&o.id).copied())
                .unwrap_or(o.transform);
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
            self.add_occurrence_mesh(g, o, &place, alpha, &looks);
        }
        // A subassembly edited in place: the rest of the top assembly.
        self.in_place_scene(g);
        self.interference_scene(g);
        self.pattern_preview_scene(g);
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
                    self.asm.sub.clear();
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
        // Feature priority / Select Faces and Edges take a feature, face or
        // edge of the component under the cursor (`gui/asm_select.rs`);
        // Part priority a part inside a subassembly (spec 3.1a).
        let part_priority = self.asm_priority() == crate::command::selection::AsmPriority::Part;
        let nested = match (pointer, hit) {
            (Some(p), Some((h, _))) if part_priority && self.asm.tool.is_none() => self
                .assembly()
                .and_then(|a| a.find(h.occurrence))
                .is_some_and(|o| {
                    crate::assembly::tree::assembly_of(&self.project, &self.linked(), o.component)
                        .is_some()
                })
                .then(|| self.asm_sub_under(p, rect))
                .flatten(),
            _ => None,
        };
        let sub_mode = !self.asm_priority().takes_components() || nested.is_some();
        let sub_hit = match (pointer, hit) {
            _ if nested.is_some() => nested,
            (Some(p), Some(_)) if sub_mode && self.asm.tool.is_none() => {
                self.asm_sub_under(p, rect)
            }
            _ => None,
        };
        if self.asm.drag.is_none() {
            let pre = self.select.prehighlight;
            let hover = hit.map(|(h, _)| h.occurrence).filter(|_| pre && !sub_mode);
            self.asm.hover = self.frame_hover(hover);
            self.asm.sub_hover = sub_hit.clone().filter(|_| pre);
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
        // A frame corner's glyph opens its menu (spec 6.1 §3.10).
        let corner_click = primary_click
            && self.asm.tool.is_none()
            && pointer.is_some_and(|p| self.frame_corner_click(p, rect));
        if primary_click && !corner_click {
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
                _ if sub_mode => self.asm_sub_click(sub_hit.clone(), ctrl),
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
                        self.asm.sub.clear();
                        self.cons.selected = None;
                    }
                    None if !ctrl => {
                        self.asm.sel.clear();
                        self.asm.sub.clear();
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
            // Edit in place; under Part Priority the part inside a
            // subassembly.
            let nested = (self.asm_priority() == crate::command::selection::AsmPriority::Part)
                .then(|| pointer.and_then(|p| self.part_hit(p, rect)))
                .flatten()
                .map(|(part, _, _)| (part.top, part.sub));
            let (top, sub) = nested.unwrap_or((h.occurrence, Vec::new()));
            self.edit_in_place(top, sub);
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
        if response.secondary_clicked() {
            self.asm.menu_at = ui.input(|i| i.pointer.interact_pos()).map(|p| (p, rect));
        }
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
            if sub_mode && self.asm.tool.is_none() {
                self.asm_sub_menu(ui, sub_hit.clone());
            } else {
                self.asm_context_menu(ui, hit.map(|(h, _)| h.occurrence))
            }
        });

        // --- 3D ---
        let state = (self.asm.sel.clone(), self.asm.hover);
        if state != self.asm.scene_state {
            self.asm.scene_state = state;
            self.scene_dirty = true;
        }
        let tints = self.frame_tints();
        if tints != self.asm.scene_frame {
            self.asm.scene_frame = tints;
            self.scene_dirty = true;
        }
        let sub = (
            self.asm.sub.clone(),
            self.asm.sub_hover.clone().or(self.asm.tree_hover.clone()),
        );
        if sub != self.asm.scene_sub {
            self.asm.scene_sub = sub;
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
        self.asm_sub_overlay(&painter, rect);
        self.frame_overlay(&painter, rect, pointer.filter(|_| on_view));
        self.frame_select_overlay(&painter, rect, pointer.filter(|_| on_view));
        self.frame_corner_overlay(&painter, rect, pointer.filter(|_| on_view));
        self.frame_corner_menu(&ctx);
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
                    self.adapt_drag(&drag);
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
                _ if self.asm_priority() == crate::command::selection::AsmPriority::Part => "Part Priority: click a part, also one inside a subassembly, to select it (Ctrl adds); right-click for its commands".into(),
                _ if self.asm_priority() == crate::command::selection::AsmPriority::Feature => "Feature Priority: click a feature of a component to select it (Ctrl adds); right-click for Appearance, Constrain, Measure".into(),
                _ if self.asm_priority() == crate::command::selection::AsmPriority::FacesEdges => "Select Faces and Edges: click a face or an edge of a component (Ctrl adds); right-click for Appearance, Constrain, Measure".into(),
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
            if self.frame_member_menu(ui, &ids) {
                ui.separator();
            }
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
    /// Constrain / Joint started from the view's menu: the face, edge or
    /// (Joint) snap point of the component under the cursor where the menu
    /// opened becomes the first selection (rule 17).
    pub(super) fn menu_first_pick(&mut self, ids: &[u32], joint: bool) {
        let Some((p, rect)) = self.asm.menu_at.take() else {
            return;
        };
        let g = if joint {
            self.joint_snap_under_pub(p, rect)
        } else {
            self.geom_under_pub(p, rect)
        };
        if let Some(g) = g.filter(|g| ids.contains(&g.occurrence())) {
            self.constrain_pick(g);
        }
    }

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
        let adaptive = all(|o| o.adaptive);
        let isolated = self.asm.isolated.is_some();
        // Inventor's first group: relate the component to another one.
        if ui.button("Constrain...").clicked() {
            self.asm_apply(Action::AsmConstrain);
            self.menu_first_pick(ids, false);
            ui.close();
        }
        if ui.button("Joint...").clicked() {
            self.asm_apply(Action::AsmJoint);
            self.menu_first_pick(ids, true);
            ui.close();
        }
        if ui.button("Measure").clicked() {
            self.apply(Action::App(super::commands::AppCmd::Measure));
            ui.close();
        }
        ui.separator();
        if ids.len() == 1 && ui.button("Edit").clicked() {
            self.edit_in_place(ids[0], Vec::new());
            ui.close();
        }
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
        let mut ad = adaptive;
        if ids.len() == 1
            && ui
                .checkbox(&mut ad, "Adaptive")
                .on_hover_text(ADAPTIVE_TIP)
                .clicked()
        {
            self.toggle_adaptive(ids[0]);
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
                let mut ad = o.adaptive;
                super::panels::plabel(ui, "Adaptive");
                if ui
                    .checkbox(&mut ad, "")
                    .on_hover_text(ADAPTIVE_TIP)
                    .changed()
                {
                    self.toggle_adaptive(o.id);
                }
                ui.end_row();
                if self
                    .assembly()
                    .is_some_and(|a| a.pattern_of(o.id).is_some())
                {
                    super::panels::plabel(ui, "Pattern");
                    self.pattern_note(ui, o.id);
                    ui.end_row();
                }
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
                // The assembly's appearance override (spec 3.0 §3.7).
                super::panels::plabel(ui, "Appearance");
                let doc = self
                    .assembly()
                    .map(|a| a.appearances.clone())
                    .unwrap_or_default();
                let now = o
                    .looks
                    .appearance
                    .clone()
                    .unwrap_or_else(|| "As Part".into());
                let mut pick = None;
                ui.menu_button(now, |ui| {
                    ui.set_max_height(480.0);
                    pick = self.asm_appearance_items(ui, &doc);
                });
                if let Some(n) = pick {
                    let t = [crate::assembly::looks::AsmLookTarget::Occurrence(o.id)];
                    self.asm_set_look(&t, n.as_deref());
                }
                ui.end_row();
                super::panels::plabel(ui, "DOF");
                let copy = self.assembly().is_some_and(|a| a.is_element(o.id));
                ui.label(if copy {
                    "0 (placed by its pattern)".to_owned()
                } else if o.grounded {
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
        // A pattern's copy is placed by its pattern.
        let copy = self.assembly().is_some_and(|a| a.is_element(o.id));
        ui.add_enabled_ui(!copy, |ui| {
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
        });
        if vals != before && !copy {
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

/// The planar face of another part under the eye ray `o`, `d` while a
/// linked part is edited in its tab (the copies taken at the start): its
/// distance, the reference a sketch on it makes, the hover.
fn linked_face_under(
    ip: &InPlace,
    le: &LinkedEdit,
    o: Point3,
    d: crate::kernel::Vector3,
) -> Option<(f64, crate::part::context::ContextRef, ContextHover)> {
    use crate::part::context::{ContextRef, ContextSource};
    let mut best: Option<(f64, usize, String, Point3, crate::kernel::FaceId)> = None;
    for (k, leaf) in le.leaves.iter().enumerate() {
        let (p, t) = &le.context[k];
        let to_leaf = t.inverse();
        let (lo, ld) = (to_leaf * o, to_leaf * d);
        let Some((f, at)) = leaf.mesh.hit(&lo, &ld) else {
            continue;
        };
        if best.as_ref().is_some_and(|b| b.0 <= at) || p.body.face_plane(f).is_none() {
            continue;
        }
        if let Some(name) = p.body.face(f).and_then(|x| x.name.clone()) {
            best = Some((at, k, name, lo + ld * at, f));
        }
    }
    let (at, k, face, q, f) = best?;
    let mut edited = vec![ip.top];
    edited.extend(&ip.sub);
    let source = ContextSource {
        assembly: ip.assembly,
        target: edited,
        path: le.leaves[k].path.clone(),
        face,
        edge: None,
        hint: [q.x, q.y, q.z],
        label: format!("{} face", le.leaves[k].name),
    };
    let (p, t) = &le.context[k];
    let geom = crate::assembly::context::geom_from(p, &source, t).ok()?;
    Some((
        at,
        ContextRef {
            id: String::new(),
            source,
            geom,
        },
        (le.leaves[k].component, f, *t),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{AssemblyDocument, DocumentData, Project};
    use crate::kernel::Vector3;
    use crate::part::PartDocument;

    /// A part edited inside a subassembly edited in place is found at the
    /// path and placement the top assembly's tree gives it (Bracket:2,
    /// turned 90°, and its second Post).
    #[test]
    fn a_part_inside_a_subassembly_in_place_is_placed_through_it() {
        let mut project = Project::new("T");
        let post = project.add_document(
            "Post",
            DocumentData::Part(PartDocument::with_box("Box1", [8.0, 8.0, 20.0]).unwrap()),
        );
        let mut bracket = AssemblyDocument::default();
        bracket.place(
            DocRef::local(post),
            "Post",
            Transform3::translation(4.0, 11.0, 5.0),
            false,
        );
        let p2 = bracket.place(
            DocRef::local(post),
            "Post",
            Transform3::translation(28.0, 11.0, 5.0),
            false,
        );
        let post_local = bracket.find(p2).unwrap().transform;
        let b = project.add_document("Bracket", DocumentData::Assembly(bracket));
        let mut top = AssemblyDocument::default();
        top.place(
            DocRef::local(b),
            "Bracket",
            Transform3::translation(10.0, 10.0, 6.0),
            false,
        );
        let turned = Transform3::new(
            Vector3::new(100.0, 20.0, 6.0),
            Vector3::z() * std::f64::consts::FRAC_PI_2,
        );
        let b2 = top.place(DocRef::local(b), "Bracket", turned, false);
        let top_id = project.add_document("Top", DocumentData::Assembly(top.clone()));
        let (t, sub, place) = nest_in(b2, &[], turned, p2, &[], post_local);
        let linked = crate::assembly::tree::Linked::new();
        let leaf = crate::assembly::tree::leaves(&project, &linked, DocRef::local(top_id), &top)
            .into_iter()
            .find(|l| l.path == [t, sub[0]])
            .expect("the leaf");
        let q = Point3::new(1.0, 2.0, 3.0);
        assert!((leaf.world * q - place * q).norm() < 1e-9);
        assert_eq!(sub, vec![p2]);
    }
}
