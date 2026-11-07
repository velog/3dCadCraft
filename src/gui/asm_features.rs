//! Assembly features (spec 3.10 §3.8, T7) in the app: Assembly Features
//! opens the hidden tools part in place (sketch on the components' faces,
//! Extrude, Hole); Return gives each new tool body the components it
//! reaches as participants; the participants are drawn cut (a cached copy
//! per occurrence, never written into the part); the browser lists the
//! features with their participants (Remove Participant, Add Selected).

use std::rc::Rc;

use super::CadCraftApp;
use crate::assembly::features;
use crate::document::{DocRef, DocumentData, DocumentId};
use crate::kernel::Transform3;
use crate::part::PartDocument;

/// Cut copies per occurrence path: (key, the copy).
pub(super) type CutCache =
    std::cell::RefCell<std::collections::HashMap<Vec<u32>, (u64, Rc<PartDocument>)>>;

impl CadCraftApp {
    /// Assembly Features: the tools part (made when there is none) edited
    /// in place.
    pub(super) fn asm_features_edit(&mut self) {
        if !self.is_assembly() {
            return;
        }
        let asm = self.part_id;
        let mut trial = self.project.clone();
        match features::ensure_tools(&mut trial, asm) {
            Ok((occ, _)) => {
                if trial.documents().len() != self.project.documents().len() {
                    self.checkpoint("Assembly Features");
                    self.project = trial;
                }
                self.edit_in_place(occ, Vec::new());
                self.file_status = Some(Ok(
                    "Assembly Features: sketch on the components and Extrude the tools (New Solid); Return cuts what they reach"
                        .into(),
                ));
            }
            Err(e) => self.file_status = Some(Err(format!("Assembly Features: {e}"))),
        }
    }

    /// Assemble › Start 2D Sketch (spec 3.10 T0): a sketch of the assembly.
    /// It lives in the tools part (placed at the origin: its coordinates are
    /// the assembly's), edited in place, so every sketch tool works and a
    /// component's face becomes a cross-part reference; Finish Sketch (or
    /// Esc before a plane is picked) returns to the assembly.
    pub(super) fn asm_start_sketch(&mut self) {
        let face = self.asm_selected_face();
        if !self.asm_open_tools("Start 2D Sketch") {
            return;
        }
        self.asm.sketch_return = true;
        self.mode = super::Mode::PickPlane(super::PickPurpose::NewSketch);
        self.file_status = Some(Ok(
            "Start 2D Sketch: pick a planar face of a component or an origin plane (Esc returns to the assembly)"
                .into(),
        ));
        // A face selected before is the sketch plane (rule 17).
        if let Some(g) = face
            && let Some((c, _)) = self.asm_face_context(&g)
        {
            self.asm.pending_context = Some(c);
            self.plane_picked(crate::part::PlaneRef::Context(String::new()));
        }
    }

    /// The one face selected in the assembly (Select: Faces and Edges), if
    /// that is the whole selection.
    fn asm_selected_face(&self) -> Option<crate::assembly::relation::GeomRef> {
        use crate::assembly::relation::GeomRef;
        match self.asm.sub.as_slice() {
            [super::asm_select::AsmSub::Geom(g @ GeomRef::Face { .. })] => Some(g.clone()),
            _ => None,
        }
    }

    /// The face `g` of a component as a cross-part reference of the part
    /// edited in place now (the tools part), with its highlight; `None`
    /// when it is not planar.
    fn asm_face_context(
        &self,
        g: &crate::assembly::relation::GeomRef,
    ) -> Option<(
        crate::part::context::ContextRef,
        super::assembly::ContextHover,
    )> {
        use crate::assembly::relation::GeomRef;
        use crate::assembly::tree;
        use crate::part::context::{ContextRef, ContextSource};
        let GeomRef::Face {
            occurrence,
            sub,
            name,
            hint,
        } = g
        else {
            return None;
        };
        let ip = self.in_place_now()?;
        if ip.linked.is_some() {
            return None;
        }
        let linked = self.linked();
        let a = tree::assembly_of(&self.project, &linked, DocRef::local(ip.assembly))?;
        let n = tree::nested(&self.project, &linked, a, *occurrence, sub).ok()?;
        let world = a.find(*occurrence)?.transform * n.at;
        let mut path = vec![*occurrence];
        path.extend(sub);
        let mut target = vec![ip.top];
        target.extend(&ip.sub);
        let source = ContextSource {
            assembly: ip.assembly,
            target,
            path,
            face: name.clone(),
            edge: None,
            hint: *hint,
            label: format!("{} face", n.name),
        };
        let geom = crate::assembly::context::current(&self.project, &linked, a, &source).ok()?;
        let f = self.with_part(n.component, |p| p.body.find_face(name))??;
        let hover = (n.component, f, ip.place.inverse() * world);
        Some((
            ContextRef {
                id: String::new(),
                source,
                geom,
            },
            hover,
        ))
    }

    /// Assemble › Work Features (spec 3.10 T0): a work plane, axis, point
    /// or UCS of the assembly, made in the tools part edited in place, so
    /// it takes the components' faces (cross-part references), the
    /// assembly's origin and its other work features; OK or Esc returns to
    /// the assembly.
    pub(super) fn asm_work(&mut self, kind: super::work::WorkKind) {
        let title = kind.title();
        let face = self.asm_selected_face();
        self.asm.work_box.set(self.assembly_box());
        if !self.asm_open_tools(title) {
            return;
        }
        self.asm.sketch_return = true;
        self.asm.work_contexts.clear();
        let mut draft = super::work::WorkDraft::new(kind);
        // A face selected before is the first input (rule 17).
        if let Some(g) = face
            && let Some(ch) = self.asm_face_context(&g)
        {
            self.asm.work_contexts.push(ch);
            draft.pick(super::work::WpInput::Plane(crate::part::PlaneRef::Context(
                "?0".into(),
            )));
        }
        self.work_draft = Some(draft);
        self.file_status = Some(Ok(format!(
            "{title}: pick faces of the components, the assembly's origin or its work features (Esc returns to the assembly)"
        )));
    }

    /// Redefine of the assembly's work feature `id` (browser).
    pub(super) fn asm_redefine_work(&mut self, id: &str) {
        self.asm.work_box.set(self.assembly_box());
        if !self.asm_open_tools("Redefine Feature") {
            return;
        }
        let Some(k) = self.part().features.iter().position(|f| f.id() == id) else {
            return;
        };
        self.asm.sketch_return = true;
        self.asm.work_contexts.clear();
        self.redefine_work(k);
    }

    /// The assembly's work features: the tools part's (id, name, kind,
    /// visible).
    pub(super) fn asm_works(&self) -> Vec<(String, String, super::work::WorkKind, bool)> {
        let Some(p) = self.asm_tools_doc() else {
            return Vec::new();
        };
        self.with_part(p, |part| {
            part.features
                .iter()
                .filter_map(|f| match f {
                    crate::part::Feature::Work(w) => Some((
                        w.id.clone(),
                        w.name.clone(),
                        super::work::WorkKind::of(&w.def),
                        w.visible,
                    )),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
    }

    /// The assembly's visible work feature under `pos` (spec 3.10 T0): a
    /// work point or axis within a few pixels, or (`planes`) a work plane
    /// the eye ray meets — as a relationship's input. A UCS element is
    /// its own input (`UCS1:XY`).
    pub(super) fn asm_work_under(
        &self,
        pos: eframe::egui::Pos2,
        rect: eframe::egui::Rect,
        planes: bool,
    ) -> Option<crate::assembly::relation::GeomRef> {
        use crate::part::{LineRef, PointRef};
        let occ = self.asm_tools_occurrence()?;
        let doc = self.asm_tools_doc()?;
        let id = self.with_part(doc, |part| {
            match self.work_axis_or_point_under_in(part, pos, rect) {
                Some(super::work::WpInput::Point(PointRef::Work(id)))
                | Some(super::work::WpInput::Line(LineRef::Work(id))) => Some(id),
                _ if planes => self.work_plane_under_in(part, pos, rect).map(|(id, _)| id),
                _ => None,
            }
        })??;
        Some(crate::assembly::relation::GeomRef::Work {
            sub: Vec::new(),
            occurrence: occ,
            id,
        })
    }

    /// The occurrence of the active assembly's tools part.
    pub(super) fn asm_tools_occurrence(&self) -> Option<u32> {
        self.assembly()?.features.as_ref().map(|f| f.occurrence)
    }

    /// Edit Sketch of the assembly's sketch `id` (browser).
    pub(super) fn asm_edit_sketch(&mut self, id: &str) {
        if !self.asm_open_tools("Edit Sketch") {
            return;
        }
        let Some(i) = self.part().sketches.iter().position(|s| s.id == id) else {
            return;
        };
        self.asm.sketch_return = true;
        self.edit_sketch(i);
    }

    /// The tools part (made when there is none, one Undo step) edited in
    /// place; `false` when it cannot be.
    fn asm_open_tools(&mut self, label: &str) -> bool {
        if !self.is_assembly() {
            return false;
        }
        let asm = self.part_id;
        let mut trial = self.project.clone();
        match features::ensure_tools(&mut trial, asm) {
            Ok((occ, doc)) => {
                if trial.documents().len() != self.project.documents().len() {
                    self.checkpoint(label);
                    self.project = trial;
                }
                self.edit_in_place(occ, Vec::new());
                self.part_id == doc
            }
            Err(e) => {
                self.file_status = Some(Err(format!("{label}: {e}")));
                false
            }
        }
    }

    /// Once a frame: an assembly sketch that ended (Finish Sketch, Esc while
    /// picking its plane) returns to the assembly.
    pub(super) fn asm_sketch_tick(&mut self) {
        if !self.asm.sketch_return {
            return;
        }
        let open = self.editing.is_some()
            || self.work_draft.is_some()
            || matches!(self.mode, super::Mode::PickPlane(_))
            || self.asm.pending_context.is_some();
        if open {
            return;
        }
        self.asm.sketch_return = false;
        self.asm.work_contexts.clear();
        if self.in_place_now().is_some() {
            self.return_to_assembly();
        }
    }

    /// The assembly's sketches: the tools part's sketches without a feature
    /// (id, name, visible).
    pub(super) fn asm_sketches(&self) -> Vec<(String, String, bool)> {
        let Some(p) = self.asm_tools_doc() else {
            return Vec::new();
        };
        self.with_part(p, |part| {
            part.sketches
                .iter()
                .filter(|s| part.consumers(&s.id).is_empty())
                .map(|s| (s.id.clone(), s.name.clone(), s.visible))
                .collect()
        })
        .unwrap_or_default()
    }

    /// The tools part of the active assembly, if it has one.
    fn asm_tools_doc(&self) -> Option<DocRef> {
        let a = self.assembly()?;
        let f = a.features.as_ref()?;
        Some(a.find(f.occurrence)?.component)
    }

    /// Draws the assembly's visible sketches in its view (the tools part
    /// stands at the origin).
    pub(super) fn asm_sketch_overlay(
        &self,
        painter: &eframe::egui::Painter,
        rect: eframe::egui::Rect,
    ) {
        let Some(p) = self.asm_tools_doc() else {
            return;
        };
        let style = super::overlay::SketchStyle {
            dim_display: self.dim_display,
            fully_color: super::theme::canvas().sketch_full,
            fully: Vec::new(),
            dof: None,
            hovered_constraint: None,
            selected_constraints: Vec::new(),
            color: eframe::egui::Color32::from_gray(90),
            reference: eframe::egui::Color32::from_gray(140),
            show_labels: false,
            selected_points: Vec::new(),
            highlighted: None,
            selected: Vec::new(),
        };
        let hovered = self.asm.work_hover.borrow_mut().take();
        self.with_part(p, |part| {
            for ps in part.sketches.iter().filter(|s| s.visible) {
                super::overlay::draw_sketch(painter, &self.camera, rect, ps, &style);
            }
            // Its work features (the tools part has no body: the
            // assembly's box sizes them).
            if part
                .features
                .iter()
                .any(|f| matches!(f, crate::part::Feature::Work(w) if w.visible))
            {
                self.asm.work_box.set(self.assembly_box());
                self.draw_work_of(painter, rect, part, hovered.as_deref(), None, None);
            }
        });
    }

    /// The assembly's sketches in the browser: double-click (or Edit Sketch)
    /// opens one; Visibility shows or hides it in the assembly.
    pub(super) fn asm_sketches_tree(&mut self, ui: &mut eframe::egui::Ui) {
        let sketches = self.asm_sketches();
        let mut edit: Option<String> = None;
        let mut toggle: Option<String> = None;
        for (id, name, visible) in &sketches {
            let r = ui
                .horizontal(|ui| {
                    super::panels::tree_icon(ui, super::icons::Icon::Sketch);
                    let text = if *visible {
                        eframe::egui::RichText::new(name)
                    } else {
                        eframe::egui::RichText::new(name).weak()
                    };
                    ui.add(eframe::egui::Label::new(text).sense(eframe::egui::Sense::click()))
                })
                .inner;
            if r.double_clicked() {
                edit = Some(id.clone());
            }
            r.context_menu(|ui| {
                if ui.button("Edit Sketch").clicked() {
                    edit = Some(id.clone());
                    ui.close();
                }
                let mut v = *visible;
                if ui.checkbox(&mut v, "Visibility").clicked() {
                    toggle = Some(id.clone());
                    ui.close();
                }
            });
        }
        if let Some(id) = toggle
            && let Some(DocRef {
                document,
                library: None,
                ..
            }) = self.asm_tools_doc()
        {
            self.checkpoint("Visibility");
            if let Some(DocumentData::Part(p)) =
                self.project.document_mut(document).map(|d| &mut d.data)
                && let Some(s) = p.sketches.iter_mut().find(|s| s.id == id)
            {
                s.visible = !s.visible;
            }
        }
        if let Some(id) = edit {
            self.asm_edit_sketch(&id);
        }
    }

    /// The assembly's work features in the browser: a click picks one for
    /// an open relationship (like a component's work features); the menu
    /// has Redefine Feature and Visibility.
    pub(super) fn asm_works_tree(&mut self, ui: &mut eframe::egui::Ui) {
        use super::work::WorkKind;
        let works = self.asm_works();
        let Some(occ) = self.asm_tools_occurrence() else {
            return;
        };
        let mut redefine: Option<String> = None;
        let mut toggle: Option<String> = None;
        let mut pick: Option<crate::assembly::relation::GeomRef> = None;
        for (id, name, kind, visible) in &works {
            let icon = match kind {
                WorkKind::Plane => super::icons::Icon::WorkPlane,
                WorkKind::Axis => super::icons::Icon::WorkAxis,
                WorkKind::Point => super::icons::Icon::WorkPoint,
                WorkKind::Ucs => super::icons::Icon::Ucs,
            };
            let g = crate::assembly::relation::GeomRef::Work {
                sub: Vec::new(),
                occurrence: occ,
                id: id.clone(),
            };
            let picked = self.cons.draft.as_ref().is_some_and(|d| d.has(&g));
            let r = ui
                .horizontal(|ui| {
                    super::panels::tree_icon(ui, icon);
                    let text = if *visible {
                        eframe::egui::RichText::new(name)
                    } else {
                        eframe::egui::RichText::new(name).weak()
                    };
                    ui.selectable_label(picked, text)
                })
                .inner;
            if r.hovered() {
                *self.asm.work_hover.borrow_mut() = Some(id.clone());
                self.cons.browser_hover = Some(g.clone());
            }
            if r.clicked() && (self.cons.draft.is_some() || self.part_planned.is_some()) {
                pick = Some(g);
            }
            if r.double_clicked() {
                redefine = Some(id.clone());
            }
            r.context_menu(|ui| {
                if ui.button("Redefine Feature").clicked() {
                    redefine = Some(id.clone());
                    ui.close();
                }
                let mut v = *visible;
                if ui.checkbox(&mut v, "Visibility").clicked() {
                    toggle = Some(id.clone());
                    ui.close();
                }
            });
        }
        if let Some(g) = pick {
            if self.part_planned.is_some() {
                self.planned_take_ref(g);
            } else {
                self.constrain_pick(g);
            }
        }
        if let Some(id) = toggle
            && let Some(DocRef {
                document,
                library: None,
                ..
            }) = self.asm_tools_doc()
        {
            self.checkpoint("Visibility");
            if let Some(DocumentData::Part(p)) =
                self.project.document_mut(document).map(|d| &mut d.data)
                && let Some(crate::part::Feature::Work(w)) =
                    p.features.iter_mut().find(|f| f.id() == id)
            {
                w.visible = !w.visible;
            }
        }
        if let Some(id) = redefine {
            self.asm_redefine_work(&id);
        }
    }

    /// After the tools part `doc` was edited: the participants of the
    /// assembly now shown follow its bodies (one Undo step).
    pub(super) fn asm_features_refresh(&mut self, doc: DocumentId) {
        let asm = self.part_id;
        let is_tools = matches!(self.project.document(asm).map(|d| &d.data),
            Some(DocumentData::Assembly(a)) if a.features.as_ref().and_then(|f| a.find(f.occurrence)).is_some_and(|o| o.component == DocRef::local(doc)));
        if !is_tools {
            return;
        }
        let linked = self.linked();
        let mut trial = self.project.clone();
        if features::refresh(&mut trial, &linked, asm) {
            self.checkpoint("Assembly Features");
            self.project = trial;
            self.scene_dirty = true;
        }
    }

    /// The tools part of the active assembly (built).
    fn asm_tools_part(&self) -> Option<Rc<PartDocument>> {
        let a = self.assembly()?;
        let f = a.features.as_ref()?;
        let o = a.find(f.occurrence)?;
        let p = self.with_part(o.component, |p| features::built(p).into_owned())?;
        Some(Rc::new(p))
    }

    /// The shown part of the occurrence at `path` (component `comp`, placed
    /// `at`) when assembly features cut it: a cached cut copy.
    pub(super) fn asm_cut(
        &self,
        path: &[u32],
        comp: DocRef,
        at: &Transform3,
    ) -> Option<Rc<PartDocument>> {
        let a = self.assembly()?;
        let f = a.features.as_ref()?;
        if !f
            .participants
            .values()
            .any(|ps| ps.iter().any(|p| p == path))
        {
            return None;
        }
        let tools_part = self.asm_tools_part()?;
        let tools = features::tools_for(a, &tools_part, path);
        if tools.is_empty() {
            return None;
        }
        let key = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            crate::part::derive::signature(&tools_part).hash(&mut h);
            self.with_part(comp, crate::part::derive::signature)
                .hash(&mut h);
            for v in at.to_homogeneous().iter() {
                v.to_bits().hash(&mut h);
            }
            for t in &tools {
                t.name.hash(&mut h);
            }
            h.finish()
        };
        if let Some((k, p)) = self.asm.cuts.borrow().get(path)
            && *k == key
        {
            return Some(p.clone());
        }
        let part = self.with_part(comp, |p| features::built(p).into_owned())?;
        let cut = features::cut(&part, at, &tools).ok().flatten()?;
        let cut = Rc::new(cut);
        self.asm
            .cuts
            .borrow_mut()
            .insert(path.to_vec(), (key, cut.clone()));
        Some(cut)
    }

    /// The browser's Features folder: each tool body with its participants
    /// (Remove Participant; Add Selected adds the selected components).
    pub(super) fn asm_features_tree(&mut self, ui: &mut eframe::egui::Ui) {
        let Some(f) = self.assembly().and_then(|a| a.features.clone()) else {
            return;
        };
        if f.participants.is_empty() {
            return;
        }
        let names: std::collections::HashMap<u32, String> = self
            .assembly()
            .map(|a| {
                a.occurrences
                    .iter()
                    .map(|o| (o.id, o.name.clone()))
                    .collect()
            })
            .unwrap_or_default();
        let mut remove: Option<(String, Vec<u32>)> = None;
        let mut add: Option<String> = None;
        let mut edit = false;
        eframe::egui::CollapsingHeader::new("Assembly Features")
            .id_salt("asm_features")
            .default_open(true)
            .show(ui, |ui| {
                for (tool, ps) in &f.participants {
                    ui.collapsing(format!("{tool} ({} cut)", ps.len()), |ui| {
                        for p in ps {
                            let label = p
                                .iter()
                                .map(|id| names.get(id).cloned().unwrap_or_else(|| id.to_string()))
                                .collect::<Vec<_>>()
                                .join("/");
                            ui.label(label).context_menu(|ui| {
                                if ui.button("Remove Participant").clicked() {
                                    remove = Some((tool.clone(), p.clone()));
                                    ui.close();
                                }
                            });
                        }
                    })
                    .header_response
                    .context_menu(|ui| {
                        if ui.button("Add Selected Components").clicked() {
                            add = Some(tool.clone());
                            ui.close();
                        }
                        if ui.button("Edit Features").clicked() {
                            edit = true;
                            ui.close();
                        }
                    });
                }
            });
        if let Some((tool, p)) = remove {
            self.edit_assembly("Remove Participant", |a| {
                if let Some(ps) = a
                    .features
                    .as_mut()
                    .and_then(|f| f.participants.get_mut(&tool))
                {
                    ps.retain(|x| *x != p);
                }
            });
            self.scene_dirty = true;
        }
        if let Some(tool) = add {
            let sel: Vec<Vec<u32>> = self.asm.sel.iter().map(|id| vec![*id]).collect();
            self.edit_assembly("Add Participant", |a| {
                if let Some(ps) = a
                    .features
                    .as_mut()
                    .and_then(|f| f.participants.get_mut(&tool))
                {
                    for s in sel {
                        if !ps.contains(&s) {
                            ps.push(s);
                        }
                    }
                }
            });
            self.scene_dirty = true;
        }
        if edit {
            self.asm_features_edit();
        }
    }
}
