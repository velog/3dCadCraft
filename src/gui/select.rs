//! Selection in the 3D view (`docs/spec/0.9-selection.md`,
//! `docs/spec/0.9a-selection-filters.md`): one model for the idle view and
//! the commands. Everything is pickable by default; the user filter
//! ([`Filter`]) switches categories off, [`Priority`] (Inventor's Select
//! Priority) decides what a click on a face, edge or vertex selects, and an
//! open command's active selector narrows it further (`pick_filter`, set
//! once per frame). Also: multiple selection with Ctrl/Shift, window /
//! crossing boxes, Select Tangencies / Loop / All / Invert / Previous, the
//! Select drop-down of the Quick Access Toolbar and the Shift + right-click
//! selection menu.

use eframe::egui::{self, Color32};

use super::{CadCraftApp, Mode, overlay};
use crate::command::flow::PickKind;
use crate::command::selection::{
    self as cs, Candidate, Category, MenuContext, MenuEntry, Priority,
};
use crate::kernel::{EdgeId, FaceId};
use crate::part::select as psel;

/// One selected object. Faces, vertices, features and bodies by persistent
/// name; edges by id (the set is cleared when the body is rebuilt).
#[derive(Clone, Debug, PartialEq)]
pub(super) enum SelItem {
    Face(String),
    Edge(EdgeId),
    Vertex(String),
    /// A feature (also a work feature) by id.
    Feature(String),
    Body(String),
    Sketch(usize),
}

impl CadCraftApp {
    /// Whether category `c` may be picked this frame (the active selector's
    /// kinds ∩ the user filter, see `update_pick_filter`).
    pub(super) fn cat_on(&self, c: Category) -> bool {
        self.pick_filter.get().filter.has(c)
    }

    /// Computes this frame's pick filter (start of `viewport`).
    pub(super) fn update_pick_filter(&self) {
        let kinds = self.active_pick_kinds();
        self.pick_filter
            .set(cs::effective(kinds.as_deref(), self.select.filter));
    }

    /// What the open command's active input takes (`None`: no command, the
    /// idle view). Commands with a selector [`crate::command::flow::Flow`]
    /// answer from it; the others are described here.
    pub(super) fn active_pick_kinds(&self) -> Option<Vec<PickKind>> {
        use PickKind as K;
        if let Some(f) = self.command_flow() {
            return Some(f.current().map(|s| s.accepts.to_vec()).unwrap_or_default());
        }
        if let Some(k) = self.coil_rib_kinds() {
            return Some(k);
        }
        if let Some(d) = &self.sweep_draft {
            return Some(match d.input {
                super::sweep::SweepInput::Profiles => vec![K::Region],
                _ => vec![K::SketchCurve, K::Edge],
            });
        }
        if let Some(d) = &self.blend_draft {
            return Some(if d.picks_faces() {
                vec![K::Face]
            } else {
                vec![K::Edge]
            });
        }
        if self.hole_draft.is_some() {
            return Some(vec![K::SketchPoint, K::Edge, K::PlanarFace, K::Face]);
        }
        if self.thread_draft.is_some() {
            return Some(vec![K::Face]);
        }
        if let Some(d) = &self.pattern_draft {
            if d.picks_geometry() {
                return Some(vec![K::Face, K::Body]);
            }
            let mut v = Vec::new();
            if d.picks_lines() {
                v.extend([K::StraightEdge, K::Axis]);
            }
            if d.picks_planes() {
                v.extend([K::PlanarFace, K::Plane]);
            }
            return Some(v);
        }
        if self.work_draft.is_some() {
            return Some(vec![
                K::Point,
                K::Axis,
                K::Edge,
                K::Face,
                K::Plane,
                K::SketchPoint,
                K::SketchCurve,
            ]);
        }
        if matches!(self.mode, Mode::PickPlane(_)) || self.prim_draft.is_some() {
            return Some(vec![K::PlanarFace, K::Plane]);
        }
        if self.target_pick.is_some() {
            return Some(vec![K::Face, K::Plane, K::Point]);
        }
        if let Some(d) = &self.extrude {
            return Some(if d.axis_pick {
                vec![K::StraightEdge, K::Axis, K::SketchCurve]
            } else {
                vec![K::Region]
            });
        }
        if self.editing.is_some() && self.project_geometry {
            return Some(vec![K::Edge, K::Face, K::Point, K::SketchCurve]);
        }
        None
    }

    /// Whether a face is selectable as a face this frame (faces still hide
    /// what is behind them when they are filtered out).
    pub(super) fn face_hit(
        &self,
        origin: &crate::kernel::Point3,
        dir: &crate::kernel::Vector3,
    ) -> Option<(FaceId, f64)> {
        if !self.cat_on(Category::Face) {
            return None;
        }
        self.picker().pick(origin, dir)
    }

    /// The raw things under the cursor with their categories, in no
    /// particular order: the edge / face already found (`edge`, `face`),
    /// vertices, sketches and work features. `all` (Select Other): also
    /// edges near the cursor that are hidden, and every face along the ray.
    fn raw_under(
        &self,
        p: egui::Pos2,
        rect: egui::Rect,
        edge: Option<EdgeId>,
        face: Option<&str>,
        all: bool,
    ) -> Vec<(SelItem, Candidate)> {
        let body = &self.part().body;
        let cand = |cat, dist: f32, surface| Candidate { cat, dist, surface };
        let mut out: Vec<(SelItem, Candidate)> = Vec::new();
        let root = |id: &str| id.split(':').next().unwrap_or(id).to_owned();
        match self.work_axis_or_point_under(p, rect) {
            Some(super::work::WpInput::Point(crate::part::PointRef::Work(id))) => {
                out.push((
                    SelItem::Feature(root(&id)),
                    cand(Category::WorkPoint, 0.0, false),
                ));
            }
            Some(super::work::WpInput::Line(crate::part::LineRef::Work(id))) => {
                out.push((
                    SelItem::Feature(root(&id)),
                    cand(Category::WorkAxis, 0.0, false),
                ));
            }
            _ => {}
        }
        if let Some((id, t)) = self.work_plane_under(p, rect) {
            out.push((
                SelItem::Feature(root(&id)),
                cand(Category::WorkPlane, t as f32, false),
            ));
        }
        if let Some(crate::part::PointRef::Vertex(name)) = self.vertex_under(p, rect) {
            let surface =
                psel::face_of_vertex(body, &name).is_some_and(|f| psel::face_on_surface(body, f));
            out.push((SelItem::Vertex(name), cand(Category::Vertex, 0.0, surface)));
        }
        if let Some(i) = self.sketch_under(p, rect) {
            // A selector of sketch curves (Boundary Patch, a Trim tool) takes the
            // sketch as its curves.
            let cat = if self.cat_on(Category::Sketch) {
                Category::Sketch
            } else {
                Category::SketchEntity
            };
            out.push((SelItem::Sketch(i), cand(cat, 0.0, false)));
        }
        let edge_surface = |e: EdgeId| {
            self.picker()
                .faces_of(e)
                .first()
                .is_some_and(|&f| psel::face_on_surface(body, f))
        };
        if let Some(e) = edge {
            out.push((SelItem::Edge(e), cand(Category::Edge, 0.0, edge_surface(e))));
        }
        if let Some(n) = face
            && let Some(f) = body.find_face(n)
        {
            out.push((
                SelItem::Face(n.to_owned()),
                cand(Category::Face, 0.0, psel::face_on_surface(body, f)),
            ));
        }
        if all {
            for (e, d) in self.edges_near(p, rect) {
                if Some(e) != edge {
                    out.push((SelItem::Edge(e), cand(Category::Edge, d, edge_surface(e))));
                }
            }
            if self.cat_on(Category::Face) {
                let mut faces: Vec<(FaceId, f32)> = Vec::new();
                if let Some((o, dir)) = self.camera.ray(p, rect)
                    && let Some(dir) = dir.try_normalize(1e-12)
                {
                    faces.extend(
                        self.picker()
                            .hits(&o, &dir)
                            .into_iter()
                            .map(|(f, t)| (f, t as f32)),
                    );
                }
                // The faces on both sides of the edges (exactly on an edge
                // the ray may hit neither).
                let edges: Vec<EdgeId> = out
                    .iter()
                    .filter_map(|(x, _)| match x {
                        SelItem::Edge(e) => Some(*e),
                        _ => None,
                    })
                    .collect();
                for e in edges {
                    faces.extend(self.picker().faces_of(e).into_iter().map(|f| (f, f32::MAX)));
                }
                for (f, t) in faces {
                    if let Some(name) = body.face(f).and_then(|x| x.name.clone())
                        && Some(name.as_str()) != face
                    {
                        out.push((
                            SelItem::Face(name),
                            cand(Category::Face, t, psel::face_on_surface(body, f)),
                        ));
                    }
                }
            }
        }
        out
    }

    /// Edges within the pick radius of `p` (also hidden ones), nearest first.
    fn edges_near(&self, p: egui::Pos2, rect: egui::Rect) -> Vec<(EdgeId, f32)> {
        if !self.cat_on(Category::Edge) {
            return Vec::new();
        }
        let mut near: Vec<(EdgeId, f32)> = Vec::new();
        for (e, pts) in self.edge_polylines().iter() {
            let d = pts
                .windows(2)
                .filter_map(|w| {
                    let a = self.camera.project(&w[0], rect)?;
                    let b = self.camera.project(&w[1], rect)?;
                    let ab = b - a;
                    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
                    Some((a + ab * t).distance(p))
                })
                .fold(f32::INFINITY, f32::min);
            if d <= overlay::EDGE_PICK_PX {
                near.push((*e, d));
            }
        }
        near.sort_by(|a, b| a.1.total_cmp(&b.1));
        near
    }

    /// The raw pick resolved to `cat` (its feature or body when the
    /// priority says so).
    fn resolve_item(&self, raw: &SelItem, cat: Category) -> Option<SelItem> {
        let body = &self.part().body;
        let face_of = || -> Option<FaceId> {
            match raw {
                SelItem::Face(n) => body.find_face(n),
                SelItem::Edge(e) => self.picker().faces_of(*e).into_iter().next(),
                SelItem::Vertex(n) => psel::face_of_vertex(body, n),
                _ => None,
            }
        };
        match cat {
            Category::Feature if !matches!(raw, SelItem::Feature(_)) => {
                let f = face_of()?;
                let name = body.face(f)?.name.clone()?;
                psel::feature_id_of_face(&name).map(|id| SelItem::Feature(id.to_owned()))
            }
            Category::SolidBody | Category::SurfaceBody => {
                psel::body_of_face(body, face_of()?).map(SelItem::Body)
            }
            _ => Some(raw.clone()),
        }
    }

    /// Resolved, filtered, ordered and without repeats.
    fn resolved(&self, raw: Vec<(SelItem, Candidate)>) -> Vec<SelItem> {
        let cands: Vec<Candidate> = raw.iter().map(|(_, c)| *c).collect();
        let f = self.pick_filter.get().filter;
        let mut out: Vec<SelItem> = Vec::new();
        for (i, cat) in cs::filter_candidates(&cands, self.select.priority, f) {
            if let Some(x) = self.resolve_item(&raw[i].0, cat)
                && !out.contains(&x)
            {
                out.push(x);
            }
        }
        out
    }

    /// What a click at `p` selects in the idle view (the edge or face
    /// already found there are passed in).
    pub(super) fn sel_item_at(
        &self,
        p: egui::Pos2,
        rect: egui::Rect,
        edge: Option<EdgeId>,
        face: Option<&str>,
    ) -> Option<SelItem> {
        self.resolved(self.raw_under(p, rect, edge, face, false))
            .into_iter()
            .next()
    }

    /// Everything Select Other offers at `p`, the default pick first.
    pub(super) fn sel_candidates(&self, p: egui::Pos2, rect: egui::Rect) -> Vec<SelItem> {
        let edge = self.edge_under(p, rect);
        let face = match self.plane_under(p, rect, false) {
            Some(crate::part::PlaneRef::Face(n)) => Some(n),
            _ => None,
        };
        self.resolved(self.raw_under(p, rect, edge, face.as_deref(), true))
    }

    /// A visible sketch with a curve under the cursor.
    pub(super) fn sketch_under(&self, p: egui::Pos2, rect: egui::Rect) -> Option<usize> {
        if !self.cat_on(Category::Sketch) && !self.cat_on(Category::SketchEntity) {
            return None;
        }
        self.part().sketches.iter().enumerate().find_map(|(i, ps)| {
            (ps.visible
                && overlay::pick_entity(&self.camera, rect, &ps.sketch, p, overlay::CURVE_SNAP_PX)
                    .is_some())
            .then_some(i)
        })
    }

    /// Faces that light up for an item.
    pub(super) fn item_faces(&self, item: &SelItem) -> Vec<FaceId> {
        let body = &self.part().body;
        match item {
            // Every piece of the visible face (a cylinder in halves).
            SelItem::Face(n) => {
                let mut out: Vec<FaceId> = Vec::new();
                for f in body
                    .faces()
                    .filter(|(_, x)| x.name.as_deref() == Some(n.as_str()))
                {
                    for g in body.smooth_group(f.0) {
                        if !out.contains(&g) {
                            out.push(g);
                        }
                    }
                }
                out
            }
            SelItem::Feature(id) => psel::faces_of_feature(body, id),
            SelItem::Body(n) => psel::faces_of_body(body, n),
            SelItem::Edge(_) | SelItem::Sketch(_) | SelItem::Vertex(_) => Vec::new(),
        }
    }

    /// The description shown in Select Other.
    pub(super) fn item_label(&self, x: &SelItem) -> String {
        let part = self.part();
        match x {
            SelItem::Edge(e) => self.edge_description(*e),
            SelItem::Face(f) => self.face_description(f),
            SelItem::Vertex(n) => format!("Vertex {n}"),
            SelItem::Feature(id) => part
                .features
                .iter()
                .find(|f| f.id() == id)
                .map_or_else(|| id.clone(), |f| format!("Feature {}", f.name())),
            SelItem::Body(n) => {
                let kind = if psel::body_is_surface(&part.body, n) {
                    "Surface Body"
                } else {
                    "Solid Body"
                };
                format!("{kind} {n}")
            }
            SelItem::Sketch(i) => part
                .sketches
                .get(*i)
                .map_or_else(|| "Sketch".to_owned(), |s| format!("Sketch {}", s.name)),
        }
    }

    /// Click in the idle view: no modifier replaces the selection, Shift
    /// adds, Ctrl toggles; a click on nothing clears it (without Ctrl/Shift).
    pub(super) fn sel_click(&mut self, item: Option<SelItem>, add: bool, toggle: bool) {
        // Measure: a plain click adds the second item (`measure_click`).
        let add = self.measure_click(add, toggle);
        if !add && !toggle {
            self.remember_selection();
            self.sel.clear();
        }
        let Some(item) = item else {
            return;
        };
        match self.sel.iter().position(|x| *x == item) {
            Some(k) if toggle => {
                self.sel.remove(k);
            }
            Some(_) => {}
            None => self.sel.push(item),
        }
    }

    /// A click (or Select Other's choice) in the idle view selects `item`:
    /// the browser and Properties follow it, and a plain click on a face or
    /// an edge shows its suggestion bar.
    pub(super) fn idle_select(
        &mut self,
        item: Option<SelItem>,
        at: egui::Pos2,
        add: bool,
        toggle: bool,
    ) {
        use super::Selection;
        self.face_sel = None;
        self.edge_sel = None;
        let feature_sel = |app: &Self, k: usize| match app.part().features[k] {
            // The base box is shown as the body (browser, Properties).
            crate::part::Feature::Box(_) => Selection::Body,
            _ => Selection::Feature(k),
        };
        let shown = match &item {
            Some(SelItem::Feature(id)) => self
                .part()
                .features
                .iter()
                .position(|f| f.id() == id)
                .map(|k| feature_sel(self, k)),
            Some(SelItem::Sketch(i)) => Some(Selection::Sketch(*i)),
            Some(SelItem::Body(_)) => Some(Selection::Body),
            Some(SelItem::Edge(e)) => self.edge_feature(*e).map(|k| feature_sel(self, k)),
            Some(SelItem::Face(n) | SelItem::Vertex(n)) => Some(
                self.face_feature(n)
                    .map_or(Selection::Body, |k| feature_sel(self, k)),
            ),
            None => None,
        };
        if let Some(s) = shown {
            self.selection = s;
        }
        if !add && !toggle {
            match &item {
                Some(SelItem::Edge(e)) => self.edge_sel = Some((*e, at)),
                Some(SelItem::Face(f)) => self.face_sel = Some((f.clone(), at)),
                _ => {}
            }
        }
        self.sel_click(item, add, toggle);
    }

    /// Keeps the current set for Previous Selection.
    fn remember_selection(&mut self) {
        if !self.sel.is_empty() {
            self.prev_sel = self.sel.clone();
        }
    }

    /// Everything Select All takes: under Faces and Edges priority the faces
    /// (or, with faces filtered out, the edges; with both out, the
    /// vertices), the visible sketches and work features; under Features /
    /// Bodies priority the features / bodies. Only categories that pass.
    fn all_items(&self) -> Vec<SelItem> {
        let part = self.part();
        let body = &part.body;
        let on = |c| self.cat_on(c);
        let mut out = Vec::new();
        let work_cat = |w: &crate::part::WorkFeature| {
            let g = &part.work;
            if g.points.contains_key(&w.id) {
                Category::WorkPoint
            } else if g.axes.contains_key(&w.id) {
                Category::WorkAxis
            } else {
                Category::WorkPlane
            }
        };
        match self.select.priority {
            Priority::FacesEdges | Priority::SketchFeatures => {
                if on(Category::Face) {
                    out.extend(
                        body.faces()
                            .filter_map(|(_, f)| f.name.clone().map(SelItem::Face)),
                    );
                } else if on(Category::Edge) {
                    out.extend(self.edge_polylines().iter().map(|(e, _)| SelItem::Edge(*e)));
                } else if on(Category::Vertex) {
                    out.extend(
                        body.vertices()
                            .filter_map(|(_, v)| v.name.clone().map(SelItem::Vertex)),
                    );
                }
            }
            Priority::Features => {
                out.extend(part.features.iter().filter_map(|f| match f {
                    crate::part::Feature::Work(_) => None,
                    _ if on(Category::Feature) => Some(SelItem::Feature(f.id().to_owned())),
                    _ => None,
                }));
            }
            Priority::Bodies => {
                out.extend(body.solids().filter_map(|(_, s)| {
                    let c = if s.open {
                        Category::SurfaceBody
                    } else {
                        Category::SolidBody
                    };
                    on(c).then(|| s.name.clone().map(SelItem::Body)).flatten()
                }));
            }
        }
        if on(Category::Sketch) {
            out.extend(
                (0..part.sketches.len())
                    .filter(|&i| part.sketches[i].visible)
                    .map(SelItem::Sketch),
            );
        }
        for f in &part.features {
            if let crate::part::Feature::Work(w) = f
                && w.visible
                && on(work_cat(w))
            {
                out.push(SelItem::Feature(w.id.clone()));
            }
        }
        out
    }

    /// Ctrl+A (Inventor: Select All, here of what the filter lets through).
    pub(super) fn select_all(&mut self) {
        self.remember_selection();
        self.sel = self.all_items();
    }

    /// Invert Selection: everything Select All takes that is not selected.
    pub(super) fn invert_selection(&mut self) {
        self.remember_selection();
        let old = std::mem::take(&mut self.sel);
        self.sel = self
            .all_items()
            .into_iter()
            .filter(|x| !old.contains(x))
            .collect();
    }

    /// Previous Selection: the set before the last change (one is kept).
    pub(super) fn previous_selection(&mut self) {
        let prev = std::mem::take(&mut self.prev_sel);
        self.prev_sel = std::mem::replace(&mut self.sel, prev);
    }

    /// Clear Selection (Esc).
    pub(super) fn clear_selection(&mut self) {
        self.remember_selection();
        self.sel.clear();
        self.face_sel = None;
        self.edge_sel = None;
    }

    /// Select Tangencies of an edge or a face (double click): adds the
    /// tangent chain of edges or the tangent faces.
    pub(super) fn select_tangencies(&mut self, item: &SelItem) {
        let body = &self.part().body;
        let more: Vec<SelItem> = match item {
            SelItem::Edge(e) => psel::tangent_edges(body, *e)
                .into_iter()
                .map(SelItem::Edge)
                .collect(),
            SelItem::Face(n) => match body.find_face(n) {
                Some(f) => psel::tangent_faces(body, f)
                    .into_iter()
                    .filter_map(|f| body.face(f).and_then(|x| x.name.clone()))
                    .map(SelItem::Face)
                    .collect(),
                None => Vec::new(),
            },
            _ => Vec::new(),
        };
        for x in more {
            if !self.sel.contains(&x) {
                self.sel.push(x);
            }
        }
    }

    /// Select Loop: the loop of the last selected edge on the face under
    /// the cursor (or its first face), or the outer loop of a selected face
    /// (or of the face `under`).
    pub(super) fn select_loop(&mut self, under: Option<&str>) {
        let body = &self.part().body;
        let under_face = under.and_then(|n| body.find_face(n));
        let edges: Vec<EdgeId> = match self.sel.last() {
            Some(SelItem::Edge(e)) => {
                let faces = self.picker().faces_of(*e);
                let f = under_face
                    .filter(|f| faces.contains(f))
                    .or_else(|| faces.first().copied());
                let near = self
                    .edge_polylines()
                    .iter()
                    .find(|(id, _)| id == e)
                    .and_then(|(_, pts)| pts.get(pts.len() / 2).copied());
                match (f, near) {
                    (Some(f), Some(near)) => psel::loop_edges(body, f, near),
                    _ => Vec::new(),
                }
            }
            Some(SelItem::Face(n)) => body
                .find_face(n)
                .map(|f| psel::outer_loop_edges(body, f))
                .unwrap_or_default(),
            _ => under_face
                .map(|f| psel::outer_loop_edges(body, f))
                .unwrap_or_default(),
        };
        for e in edges {
            let x = SelItem::Edge(e);
            if !self.sel.contains(&x) {
                self.sel.push(x);
            }
        }
    }

    /// Objects taken by a box from `a` to `b`: window takes those fully
    /// inside, crossing also those it touches (by direction, or always one
    /// of them: [`BoxMode`]). Only what the filter and priority allow.
    pub(super) fn box_items(&self, rect: egui::Rect, a: egui::Pos2, b: egui::Pos2) -> Vec<SelItem> {
        let r = egui::Rect::from_two_pos(a, b);
        let window = self.select.box_mode.is_window(a.x, b.x);
        let taken = |pts: &[egui::Pos2]| {
            !pts.is_empty()
                && if window {
                    pts.iter().all(|p| r.contains(*p))
                } else {
                    pts.iter().any(|p| r.contains(*p))
                }
        };
        let picker = self.picker();
        let face_pts = |faces: &[FaceId]| -> Vec<egui::Pos2> {
            picker
                .tris
                .iter()
                .filter(|(f, _)| faces.contains(f))
                .flat_map(|(_, t)| t.iter())
                .filter_map(|q| self.camera.project(q, rect))
                .collect()
        };
        let take = cs::box_takes(self.select.priority, self.pick_filter.get().filter);
        let part = self.part();
        let body = &part.body;
        let mut out = Vec::new();
        if take.edges {
            for (e, pts) in self.edge_polylines().iter() {
                let s: Vec<_> = pts
                    .iter()
                    .filter_map(|q| self.camera.project(q, rect))
                    .collect();
                if taken(&s) {
                    out.push(SelItem::Edge(*e));
                }
            }
        }
        if take.vertices {
            for (_, v) in body.vertices() {
                if let Some(n) = &v.name
                    && let Some(s) = self.camera.project(&v.point, rect)
                    && r.contains(s)
                {
                    out.push(SelItem::Vertex(n.clone()));
                }
            }
        }
        if take.sketches {
            for (i, ps) in part.sketches.iter().enumerate() {
                if !ps.visible {
                    continue;
                }
                let sk = &ps.sketch;
                let s: Vec<_> = sk
                    .entities()
                    .flat_map(|(id, _)| overlay::polyline(sk, id))
                    .filter_map(|q| self.camera.project(&sk.plane.point_at(q.x, q.y), rect))
                    .collect();
                if taken(&s) {
                    out.push(SelItem::Sketch(i));
                }
            }
        }
        // Faces, features or bodies: all of their triangles (window) or any.
        let mut solid_items: Vec<SelItem> = Vec::new();
        if take.faces {
            solid_items.extend(
                body.faces()
                    .filter_map(|(_, f)| f.name.clone().map(SelItem::Face)),
            );
        }
        if take.features {
            solid_items.extend(
                part.features
                    .iter()
                    .filter(|f| !matches!(f, crate::part::Feature::Work(_)))
                    .map(|f| SelItem::Feature(f.id().to_owned())),
            );
        }
        if take.solids || take.surfaces {
            solid_items.extend(
                body.solids()
                    .filter(|(_, s)| if s.open { take.surfaces } else { take.solids })
                    .filter_map(|(_, s)| s.name.clone().map(SelItem::Body)),
            );
        }
        for item in solid_items {
            if taken(&face_pts(&self.item_faces(&item))) {
                out.push(item);
            }
        }
        out
    }

    /// Applies a finished box: replaces, or adds with Shift, toggles with Ctrl.
    pub(super) fn sel_box(&mut self, items: Vec<SelItem>, add: bool, toggle: bool) {
        if !add && !toggle {
            self.remember_selection();
            self.sel.clear();
        }
        for x in items {
            match self.sel.iter().position(|y| *y == x) {
                Some(k) if toggle => {
                    self.sel.remove(k);
                }
                Some(_) => {}
                None => self.sel.push(x),
            }
        }
    }

    /// Draws the selected edges, vertices and sketches (blue) and the one
    /// under the cursor (green); faces are coloured in the 3D scene
    /// (`build_scene`, with depth).
    pub(super) fn draw_selection(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        hover: Option<&SelItem>,
    ) {
        let polys = self.edge_polylines();
        // Edges are drawn on top of the view, without their hidden pieces;
        // every kernel piece of the visible edge (a circle in arcs).
        let body = &self.part().body;
        let line = |e: EdgeId, color: Color32| {
            for g in body.edge_group(e) {
                if let Some((_, pts)) = polys.iter().find(|(id, _)| *id == g) {
                    self.draw_visible_polyline(painter, rect, pts, egui::Stroke::new(4.0, color));
                }
            }
        };
        let sketch = |i: usize, color: Color32| {
            if let Some(ps) = self.part().sketches.get(i) {
                let sk = &ps.sketch;
                for (id, _) in sk.entities() {
                    let s: Vec<_> = overlay::polyline(sk, id)
                        .iter()
                        .filter_map(|q| self.camera.project(&sk.plane.point_at(q.x, q.y), rect))
                        .collect();
                    painter.add(egui::Shape::line(s, egui::Stroke::new(3.0, color)));
                }
            }
        };
        let vertex = |n: &str, color: Color32| {
            let body = &self.part().body;
            if let Some((_, v)) = body.vertices().find(|(_, v)| v.name.as_deref() == Some(n))
                && !self.point_hidden(&v.point, rect)
                && let Some(s) = self.camera.project(&v.point, rect)
            {
                painter.circle_filled(s, 5.0, color);
            }
        };
        let blue = Color32::from_rgb(40, 110, 230);
        let green = Color32::from_rgb(60, 190, 90);
        for x in &self.sel {
            match x {
                SelItem::Edge(e) => line(*e, blue),
                SelItem::Sketch(i) => sketch(*i, blue),
                SelItem::Vertex(n) => vertex(n, blue),
                _ => {}
            }
        }
        if self.select.prehighlight
            && let Some(h) = hover
            && !self.sel.contains(h)
        {
            match h {
                SelItem::Edge(e) => line(*e, green),
                SelItem::Sketch(i) => sketch(*i, green),
                SelItem::Vertex(n) => vertex(n, green),
                _ => {}
            }
        }
    }

    /// Whether model point `q` is hidden behind a face (the eye ray through
    /// it hits a face clearly in front). Very large bodies are not tested.
    pub(super) fn point_hidden(&self, q: &crate::kernel::Point3, rect: egui::Rect) -> bool {
        let picker = self.picker();
        if picker.tris.len() >= 200_000 {
            return false;
        }
        let Some(s) = self.camera.project(q, rect) else {
            return true;
        };
        let Some((o, d)) = self.camera.ray(s, rect) else {
            return false;
        };
        let Some(d) = d.try_normalize(1e-12) else {
            return false;
        };
        let te = (q - o).dot(&d);
        picker
            .pick(&o, &d)
            .is_some_and(|(_, ft)| ft < te - 0.01 * te.abs().max(1.0))
    }

    /// Draws a model polyline on top of the view, its hidden pieces left
    /// out (each segment judged by its middle).
    pub(super) fn draw_visible_polyline(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        pts: &[crate::kernel::Point3],
        stroke: egui::Stroke,
    ) {
        for w in pts.windows(2) {
            if self.point_hidden(&(w[0] + (w[1] - w[0]) * 0.5), rect) {
                continue;
            }
            if let (Some(a), Some(b)) = (
                self.camera.project(&w[0], rect),
                self.camera.project(&w[1], rect),
            ) {
                painter.line_segment([a, b], stroke);
            }
        }
    }

    /// "3 faces, 1 edge" for the status bar.
    pub(super) fn selection_summary(&self) -> Option<String> {
        if self.sel.is_empty() {
            return None;
        }
        let count = |f: fn(&SelItem) -> bool| self.sel.iter().filter(|x| f(x)).count();
        let parts = [
            (count(|x| matches!(x, SelItem::Face(_))), "face", "faces"),
            (count(|x| matches!(x, SelItem::Edge(_))), "edge", "edges"),
            (
                count(|x| matches!(x, SelItem::Vertex(_))),
                "vertex",
                "vertices",
            ),
            (
                count(|x| matches!(x, SelItem::Feature(_))),
                "feature",
                "features",
            ),
            (count(|x| matches!(x, SelItem::Body(_))), "body", "bodies"),
            (
                count(|x| matches!(x, SelItem::Sketch(_))),
                "sketch",
                "sketches",
            ),
        ];
        let text: Vec<String> = parts
            .iter()
            .filter(|(n, _, _)| *n > 0)
            .map(|(n, one, many)| format!("{n} {}", if *n == 1 { one } else { many }))
            .collect();
        Some(format!("Selected: {}", text.join(", ")))
    }

    /// The status-bar note when the user filter hid everything the active
    /// input takes and was ignored.
    pub(super) fn filter_override_note(&self) -> Option<String> {
        if !self.pick_filter.get().overridden {
            return None;
        }
        let takes: Vec<&str> = self
            .pick_filter
            .get()
            .filter
            .categories()
            .into_iter()
            .map(Category::label)
            .collect();
        Some(format!(
            "Selection filter ignored: this input takes only {}",
            takes.join(", ")
        ))
    }

    /// Sets the priority (the selection's kind changes, so it is cleared).
    fn set_priority(&mut self, p: Priority) {
        if self.select.priority != p {
            self.clear_selection();
        }
        self.select.priority = p;
    }

    /// The Select drop-down's title: "Select: Faces and Edges" (+ " *"
    /// when a filter is off).
    fn select_menu_title(&self) -> String {
        format!(
            "Select: {}{}",
            self.select.priority.label().trim_start_matches("Select "),
            if self.select.filter.is_all() {
                ""
            } else {
                " *"
            }
        )
    }

    /// The Select drop-down of the Quick Access Toolbar (also usable while a
    /// command is open): priorities, filters, box mode, prehighlight and
    /// the selection-set tools.
    pub(super) fn select_menu(&mut self, ui: &mut egui::Ui) {
        let title = self.select_menu_title();
        ui.menu_button(title, |ui| {
            let ctx = MenuContext::Idle {
                selection: !self.sel.is_empty(),
                tangent_source: self.tangent_source().is_some(),
                loop_source: matches!(self.sel.last(), Some(SelItem::Edge(_) | SelItem::Face(_))),
                previous: !self.prev_sel.is_empty(),
                overlapping: false,
            };
            let entries: Vec<(MenuEntry, bool)> = cs::shift_menu(&ctx)
                .into_iter()
                .filter(|(e, _)| !matches!(e, MenuEntry::SelectOther))
                .collect();
            self.menu_entries(ui, &entries, None);
        });
    }

    /// The selected edge or face Select Tangencies starts from.
    fn tangent_source(&self) -> Option<SelItem> {
        self.sel
            .last()
            .filter(|x| matches!(x, SelItem::Edge(_) | SelItem::Face(_)))
            .cloned()
    }

    /// Draws menu entries (shared by the QAT drop-down and the Shift +
    /// right-click menu); returns whether one was chosen (the menu then
    /// closes). `at`: where the Shift + right-click menu opened (Select
    /// Other, Select Loop on the face there).
    fn menu_entries(
        &mut self,
        ui: &mut egui::Ui,
        entries: &[(MenuEntry, bool)],
        at: Option<(egui::Pos2, egui::Rect)>,
    ) -> bool {
        let mut done = false;
        let mut section = 0u8;
        let sec = |e: &MenuEntry| match e {
            MenuEntry::Priority(_) => 1,
            MenuEntry::Filter(_) | MenuEntry::AllFilters => 2,
            MenuEntry::Box(_) => 3,
            MenuEntry::Prehighlight => 4,
            _ => 0,
        };
        let filters: Vec<Category> = entries
            .iter()
            .filter_map(|(e, _)| match e {
                MenuEntry::Filter(c) => Some(*c),
                _ => None,
            })
            .collect();
        for (e, enabled) in entries {
            let s = sec(e);
            if s != section {
                ui.separator();
                section = s;
            }
            match e {
                MenuEntry::Filter(_) => continue,
                MenuEntry::AllFilters => {
                    // One sub-menu with the check boxes (they keep it open).
                    let title = if filters.iter().all(|c| self.select.filter.has(*c)) {
                        "Filters: All".to_owned()
                    } else {
                        "Filters: Custom".to_owned()
                    };
                    ui.menu_button(title, |ui| {
                        for c in &filters {
                            let mut on = self.select.filter.has(*c);
                            if ui.checkbox(&mut on, c.label()).changed() {
                                self.select.filter.set(*c, on);
                            }
                        }
                        ui.separator();
                        if ui.button("Select All Filters").clicked() {
                            for c in &filters {
                                self.select.filter.set(*c, true);
                            }
                        }
                    });
                    continue;
                }
                _ => {}
            }
            let clicked = match e {
                MenuEntry::Priority(p) => ui
                    .add_enabled(
                        *enabled,
                        egui::RadioButton::new(self.select.priority == *p, p.label()),
                    )
                    .clicked(),
                MenuEntry::Box(b) => ui
                    .add_enabled(
                        *enabled,
                        egui::RadioButton::new(self.select.box_mode == *b, b.label()),
                    )
                    .clicked(),
                MenuEntry::Prehighlight => {
                    let mut on = self.select.prehighlight;
                    let r = ui.checkbox(&mut on, "Enable Prehighlight");
                    if r.changed() {
                        self.select.prehighlight = on;
                    }
                    r.changed()
                }
                _ => ui
                    .add_enabled(*enabled, egui::Button::new(entry_label(e)))
                    .clicked(),
            };
            if !clicked {
                continue;
            }
            done = true;
            match e {
                MenuEntry::Priority(p) => self.set_priority(*p),
                MenuEntry::Box(b) => self.select.box_mode = *b,
                MenuEntry::Prehighlight => {}
                MenuEntry::SelectOther => {
                    if let Some((p, rect)) = at {
                        let items = self.sel_candidates(p, rect);
                        if !items.is_empty() {
                            self.select_other = Some(super::SelectOther {
                                at: p,
                                items,
                                index: 0,
                            });
                        }
                    }
                }
                MenuEntry::SelectLoop => {
                    let face = at.and_then(|(p, rect)| match self.plane_under(p, rect, false) {
                        Some(crate::part::PlaneRef::Face(n)) => Some(n),
                        _ => None,
                    });
                    self.select_loop(face.as_deref());
                }
                MenuEntry::SelectTangencies => {
                    if let Some(t) = self.tangent_source() {
                        self.select_tangencies(&t);
                    }
                }
                MenuEntry::SelectInvert => {
                    if let Some(i) = self.editing {
                        let sk = &self.part().sketches[i].sketch;
                        let all: Vec<_> = sk.entities().map(|(id, _)| id).collect();
                        let old = std::mem::take(&mut self.tools.selected);
                        self.tools.selected =
                            all.into_iter().filter(|x| !old.contains(x)).collect();
                    } else {
                        self.invert_selection();
                    }
                }
                MenuEntry::SelectAll => {
                    if let Some(i) = self.editing {
                        self.tools.selected = self.part().sketches[i]
                            .sketch
                            .entities()
                            .map(|(id, _)| id)
                            .collect();
                    } else {
                        self.select_all();
                    }
                }
                MenuEntry::PreviousSelection => self.previous_selection(),
                MenuEntry::ClearSelection => {
                    if self.editing.is_some() {
                        self.tools.selected.clear();
                        self.tools.selected_constraints.clear();
                    } else {
                        self.clear_selection();
                    }
                }
                MenuEntry::Filter(_) | MenuEntry::AllFilters => {}
            }
        }
        if done {
            ui.close();
        }
        done
    }

    /// The context of the Shift + right-click menu at `p`.
    pub(super) fn shift_menu_context(&self, p: egui::Pos2, rect: egui::Rect) -> MenuContext {
        if self.editing.is_some() && !self.project_geometry {
            return MenuContext::Sketch {
                selection: !self.tools.selected.is_empty()
                    || !self.tools.selected_constraints.is_empty(),
            };
        }
        if let Some(kinds) = self.active_pick_kinds() {
            return MenuContext::Command {
                kinds,
                overlapping: false,
            };
        }
        let under_face = matches!(
            self.plane_under(p, rect, false),
            Some(crate::part::PlaneRef::Face(_))
        );
        MenuContext::Idle {
            selection: !self.sel.is_empty(),
            tangent_source: self.tangent_source().is_some(),
            loop_source: matches!(self.sel.last(), Some(SelItem::Edge(_) | SelItem::Face(_)))
                || under_face,
            previous: !self.prev_sel.is_empty(),
            overlapping: self.sel_candidates(p, rect).len() > 1,
        }
    }

    /// Draws the open Shift + right-click selection menu at its place;
    /// closes it on a choice, Esc or a click elsewhere.
    pub(super) fn shift_menu_ui(&mut self, ctx: &egui::Context) {
        let Some((at, rect, menu)) = self.sel_menu.clone() else {
            return;
        };
        let entries = cs::shift_menu(&menu);
        let mut chosen = false;
        let area = egui::Area::new(egui::Id::new("selection_menu"))
            .fixed_pos(at)
            .order(egui::Order::Foreground)
            .constrain(true)
            .show(ctx, |ui| {
                egui::Frame::menu(ui.style()).show(ui, |ui| {
                    ui.set_min_width(190.0);
                    ui.label(egui::RichText::new("Selection").strong());
                    chosen = self.menu_entries(ui, &entries, Some((at, rect)));
                });
            });
        let clicked_out = ctx.input(|i| i.pointer.any_pressed())
            && ctx
                .pointer_interact_pos()
                .is_some_and(|p| !area.response.rect.contains(p))
            && !ctx.any_popup_open();
        let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        // The frame that opened it saw the right press itself.
        if chosen || escape || (clicked_out && !self.sel_menu_fresh) {
            self.sel_menu = None;
        }
        self.sel_menu_fresh = false;
    }
}

/// The text of a plain menu entry.
fn entry_label(e: &MenuEntry) -> &'static str {
    match e {
        MenuEntry::SelectOther => "Select Other",
        MenuEntry::SelectLoop => "Select Loop",
        MenuEntry::SelectTangencies => "Select Tangencies",
        MenuEntry::SelectInvert => "Select Invert",
        MenuEntry::SelectAll => "Select All (Ctrl+A)",
        MenuEntry::PreviousSelection => "Previous Selection",
        MenuEntry::ClearSelection => "Clear Selection (Esc)",
        MenuEntry::AllFilters => "Filters",
        MenuEntry::Prehighlight => "Enable Prehighlight",
        MenuEntry::Box(b) => b.label(),
        MenuEntry::Filter(c) => c.label(),
        MenuEntry::Priority(p) => p.label(),
    }
}
