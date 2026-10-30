//! Sketch → Create → **Project Geometry** (`docs/spec/1.8-project-geometry.md`).
//!
//! While the command is on (ribbon toggle; Esc or right-click → Done ends
//! it), a click in the view projects into the edited sketch, in this order
//! of priority:
//! 1. a point of an earlier sketch (centre points, line ends, spline
//!    points), within [`overlay::POINT_SNAP_PX`];
//! 2. a curve of an earlier sketch (line, circle, arc, spline), within
//!    [`overlay::CURVE_SNAP_PX`];
//! 3. work points and axes;
//! 4. a model vertex (a fixed point, `PartDocument::project_point`);
//! 5. a model edge (`PartDocument::project_model_edge`): straight edges by
//!    their vertices, circles / arcs as linked circles / arcs, other curves
//!    (and circles in a tilted plane) as linked splines, ends shared with
//!    the projected vertices so a chain of edges closes;
//! 6. a face: all its loops (`PartDocument::project_face`);
//! 7. a work plane (its line across the sketch).
//!
//! Earlier sketches are drawn while the command runs, also hidden (consumed)
//! ones, so their geometry can be picked. Shift+click projects as
//! construction geometry (as construction-off, with Construction on).
//! **Replace Reference** (References row or the context menu) arms
//! [`CadCraftApp::relink`]: the next pick replaces that link's source.

use eframe::egui::{self, Color32, Painter, Pos2, Rect, Stroke};

use super::{CadCraftApp, overlay};
use crate::kernel::EdgeId;
use crate::part::{LinkSource, SketchGeo};

/// What a Project Geometry click would take.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum ProjPick {
    /// Geometry of sketch `sketch` (index).
    Sketch {
        /// Source sketch index.
        sketch: usize,
        /// The point or curve.
        geo: SketchGeo,
    },
    /// A model vertex (by name).
    Vertex(crate::part::PointRef),
    /// A model edge.
    Edge(EdgeId),
    /// A model face (by name): all its loops.
    Face(String),
    /// A work point / axis / plane (by id).
    Work(String),
    /// An edge of another component while the part is edited in place: the
    /// cross-part reference it makes and the edge in this part's
    /// coordinates (spec 3.10 §3.3).
    Context(
        Box<crate::part::context::ContextRef>,
        Vec<crate::kernel::Point3>,
    ),
    /// A whole planar face of another component (every edge of its loops):
    /// the face's reference (its source names the face) and the face as
    /// highlighted.
    ContextFace(
        Box<crate::part::context::ContextRef>,
        super::assembly::ContextHover,
    ),
    /// One loop of a face of the part (Ctrl + an edge of it).
    Loop(Vec<EdgeId>),
    /// One loop of another component's face (Ctrl + an edge of it): one
    /// cross-part reference per edge.
    ContextLoop(Vec<crate::part::context::ContextRef>),
}

impl CadCraftApp {
    /// The sketches the edited sketch `i` may project from: earlier in the
    /// history and not rolled back.
    pub(super) fn project_sources(&self, i: usize) -> Vec<usize> {
        let part = self.part();
        (0..part.sketches.len())
            .filter(|&j| part.can_project_from(i, j) && !part.sketch_rolled_back(j))
            .collect()
    }

    /// The geometry a Project Geometry click at `p` would take; with
    /// `whole_loop` (Ctrl) an edge of another component stands for its loop.
    pub(super) fn project_pick(
        &self,
        i: usize,
        p: Pos2,
        rect: Rect,
        whole_loop: bool,
    ) -> Option<ProjPick> {
        let part = self.part();
        let sources = self.project_sources(i);
        let mut best: Option<(f32, ProjPick)> = None;
        for &j in &sources {
            let sk = &part.sketches[j].sketch;
            for (id, q) in sk.points() {
                let Some(s) = self.camera.project(&sk.to_world(&q.position), rect) else {
                    continue;
                };
                let d = s.distance(p);
                if d <= overlay::POINT_SNAP_PX && best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                    let geo = SketchGeo::Point { point: id };
                    best = Some((d, ProjPick::Sketch { sketch: j, geo }));
                }
            }
        }
        if let Some((_, pick)) = best {
            return Some(pick);
        }
        for &j in &sources {
            let sk = &part.sketches[j].sketch;
            if let Some((e, at)) =
                overlay::pick_entity(&self.camera, rect, sk, p, overlay::CURVE_SNAP_PX)
            {
                let d = self
                    .camera
                    .project(&sk.to_world(&at), rect)
                    .map_or(f32::MAX, |s| s.distance(p));
                if best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                    let geo = SketchGeo::Curve { entity: e };
                    best = Some((d, ProjPick::Sketch { sketch: j, geo }));
                }
            }
        }
        if let Some((_, pick)) = best {
            return Some(pick);
        }
        // Work points and axes are thin: they win over the edges under
        // them; a work plane only where no edge is.
        if let Some(id) = self.work_feature_under(p, rect)
            && !self.part().work.planes.contains_key(&id)
        {
            return Some(ProjPick::Work(id));
        }
        if let Some(v @ crate::part::PointRef::Vertex(_)) = self.vertex_under(p, rect) {
            return Some(ProjPick::Vertex(v));
        }
        if let Some(e) = self.edge_under(p, rect) {
            if whole_loop {
                let normal = self.part().sketches[i].sketch.plane.normal;
                let edges = edge_loop_on(&self.part().body, e, &normal);
                if edges.len() > 1 {
                    return Some(ProjPick::Loop(edges));
                }
            }
            return Some(ProjPick::Edge(e));
        }
        // An edge of another component in front of the part's own faces.
        if let Some((c, pts)) = self.context_edge_under(p, rect) {
            if whole_loop {
                let normal = self.part().sketches[i].sketch.plane.normal.into_inner();
                if let Ok(refs) = self.context_edge_loop(&c.source, normal) {
                    return Some(ProjPick::ContextLoop(refs));
                }
            }
            return Some(ProjPick::Context(Box::new(c), pts));
        }
        if let Some((c, hover)) = self.context_target_under(p, rect) {
            return Some(ProjPick::ContextFace(Box::new(c), hover));
        }
        if let Some((o, d)) = self.camera.ray(p, rect)
            && let Some(d) = d.try_normalize(1e-12)
            && let Some((f, _)) = self.face_hit(&o, &d)
            && let Some(name) = self.part().body.face(f).and_then(|x| x.name.clone())
            // The sketch's own face: its edges are projected already.
            && self.part().sketches[i].placement.plane != crate::part::PlaneRef::Face(name.clone())
        {
            return Some(ProjPick::Face(name));
        }
        self.work_feature_under(p, rect).map(ProjPick::Work)
    }

    /// Project Geometry: a click at `p` projects what is under it into
    /// sketch `i` (one Undo step), or replaces the source of the armed
    /// Replace Reference. Returns whether the click was taken.
    pub(super) fn project_click(
        &mut self,
        i: usize,
        p: Pos2,
        rect: Rect,
        shift: bool,
        ctrl: bool,
    ) -> bool {
        let Some(pick) = self.project_pick(i, p, rect, ctrl) else {
            return false;
        };
        if let Some((si, k)) = self.relink {
            if si == i {
                let source = match pick {
                    ProjPick::Sketch { sketch, geo } => Some(LinkSource::Sketch {
                        sketch: self.part().sketches[sketch].id.clone(),
                        geo,
                    }),
                    ProjPick::Edge(e) => crate::part::blend::edge_ref(&self.part().body, e)
                        .map(|edge| LinkSource::Edge { edge }),
                    ProjPick::Work(id) => Some(LinkSource::Work { id }),
                    ProjPick::Vertex(_)
                    | ProjPick::Face(_)
                    | ProjPick::Loop(_)
                    | ProjPick::Context(..)
                    | ProjPick::ContextFace(..)
                    | ProjPick::ContextLoop(_) => None,
                };
                let Some(source) = source else {
                    self.file_status = Some(Err(
                        "Replace Reference: pick a sketch point or curve, a named edge or a work feature".into(),
                    ));
                    return true;
                };
                self.checkpoint("Replace Reference");
                match self.part_mut().relink(i, k, source) {
                    Ok(()) => {
                        self.relink = None;
                        self.project_geometry = false;
                        self.file_status = None;
                        self.solve();
                        self.scene_dirty = true;
                    }
                    Err(e) => {
                        self.history.cancel();
                        self.file_status = Some(Err(e.to_string()));
                    }
                }
                return true;
            }
            self.relink = None;
        }
        // Inventor: Shift projects as construction (or as normal geometry
        // while Construction is on).
        let construction = shift != self.tools.construction;
        self.checkpoint("Project Geometry");
        let r = match pick {
            ProjPick::Sketch { sketch, geo } => {
                let id = self.part().sketches[sketch].id.clone();
                self.part_mut()
                    .project_sketch_geometry(i, &id, geo, construction)
                    .map(|_| ())
            }
            ProjPick::Vertex(v) => self.part_mut().project_point(i, v).map(|_| ()),
            ProjPick::Edge(e) => self
                .part_mut()
                .project_model_edge(i, e, construction)
                .map(|_| ()),
            ProjPick::Face(f) => self
                .part_mut()
                .project_face(i, &f, construction)
                .map(|_| ()),
            ProjPick::Work(id) => self.part_mut().project_work(i, &id).map(|_| ()),
            ProjPick::Context(c, _) => self
                .part_mut()
                .project_context_edge(i, *c, construction)
                .map(|_| ()),
            ProjPick::Loop(edges) => {
                // All or nothing.
                let before = self.part().sketches[i].clone();
                let mut r = Ok(());
                for e in edges {
                    if let Err(err) = self.part_mut().project_model_edge(i, e, construction) {
                        r = Err(err);
                        break;
                    }
                }
                if r.is_err() {
                    self.part_mut().sketches[i] = before;
                }
                r
            }
            ProjPick::ContextLoop(edges) => self
                .part_mut()
                .project_context_edges(i, edges, construction)
                .map(|_| ()),
            ProjPick::ContextFace(c, _) => match self.context_face_outline(&c.source) {
                Ok(edges) => self
                    .part_mut()
                    .project_context_edges(i, edges, construction)
                    .map(|_| ()),
                Err(e) => Err(crate::part::PartError::Feature(format!(
                    "Project Geometry: {e}"
                ))),
            },
        };
        match r {
            Ok(()) => {
                // Cross-part geometry projection off: a copy at once.
                if !self.in_place_opts.associative && self.part().sketch_uses_context(i) {
                    let _ = self.part_mut().copy_sketch_context(i);
                }
                let _ = self.part_mut().sketches[i].sketch.solve();
                self.file_status = None;
                self.solve();
                self.scene_dirty = true;
            }
            Err(err) => {
                self.history.cancel();
                self.file_status = Some(Err(err.to_string()));
            }
        }
        true
    }

    /// **Project Cut Edges** into the edited sketch (one Undo step).
    pub(super) fn project_cut_edges(&mut self) {
        let Some(i) = self.editing else {
            self.file_status = Some(Err("Project Cut Edges: edit a sketch first".into()));
            return;
        };
        self.checkpoint("Project Cut Edges");
        let r = self.part_mut().project_cut_edges(i).map(|_| ());
        self.projected(r);
    }

    /// **Project Flat Pattern** into the edited sketch (one Undo step): the
    /// first sheet body whose base panel is parallel to the sketch.
    pub(super) fn project_flat_pattern(&mut self) {
        let Some(i) = self.editing else {
            self.file_status = Some(Err("Project Flat Pattern: edit a sketch first".into()));
            return;
        };
        let mut bodies: Vec<String> = self.part().sheet_models.keys().cloned().collect();
        bodies.sort();
        if bodies.is_empty() {
            self.file_status = Some(Err(
                "Project Flat Pattern: the part has no sheet metal body".into(),
            ));
            return;
        }
        self.checkpoint("Project Flat Pattern");
        let mut r = Err(crate::part::PartError::Feature(String::new()));
        for b in &bodies {
            r = self.part_mut().project_flat_pattern(i, b).map(|_| ());
            if r.is_ok() {
                break;
            }
        }
        self.projected(r);
    }

    /// After a projection command: solve and redraw, or undo the checkpoint
    /// and show the error.
    fn projected(&mut self, r: Result<(), crate::part::PartError>) {
        match r {
            Ok(()) => {
                if let Some(i) = self.editing {
                    let _ = self.part_mut().sketches[i].sketch.solve();
                }
                self.file_status = None;
                self.solve();
                self.scene_dirty = true;
            }
            Err(err) => {
                self.history.cancel();
                self.file_status = Some(Err(err.to_string()));
            }
        }
    }

    /// Lights up what a Project Geometry click at `pointer` would take.
    pub(super) fn draw_project_hover(&self, painter: &Painter, rect: Rect, pointer: Option<Pos2>) {
        let (Some(i), Some(p)) = (self.editing, pointer) else {
            return;
        };
        if !self.project_geometry {
            return;
        }
        let col = overlay::HIGHLIGHT;
        let ctrl = painter.ctx().input(|i| i.modifiers.command);
        match self.project_pick(i, p, rect, ctrl) {
            Some(ProjPick::Sketch { sketch, geo }) => {
                let sk = &self.part().sketches[sketch].sketch;
                match geo {
                    SketchGeo::Point { point } => {
                        if let Some(s) = sk
                            .point(point)
                            .and_then(|q| self.camera.project(&sk.to_world(&q.position), rect))
                        {
                            painter.circle_stroke(s, 7.0, Stroke::new(2.5, col));
                        }
                    }
                    SketchGeo::Curve { entity } => {
                        let pts: Vec<Pos2> = overlay::polyline(sk, entity)
                            .into_iter()
                            .filter_map(|q| self.camera.project(&sk.to_world(&q), rect))
                            .collect();
                        painter.add(egui::Shape::line(pts, Stroke::new(3.5, col)));
                    }
                }
            }
            Some(ProjPick::Loop(edges)) => {
                let opts = crate::kernel::TessellationOptions::default();
                for e in edges {
                    if let Ok(pts) = self.part().body.sample_edge(e, &opts) {
                        let pts: Vec<Pos2> = pts
                            .iter()
                            .filter_map(|q| self.camera.project(q, rect))
                            .collect();
                        painter.add(egui::Shape::line(pts, Stroke::new(3.5, col)));
                    }
                }
            }
            Some(ProjPick::ContextLoop(edges)) => {
                for c in &edges {
                    let Some(curve) = c.geom.curve() else {
                        continue;
                    };
                    let pts: Vec<Pos2> = curve
                        .polyline()
                        .iter()
                        .filter_map(|q| self.camera.project(q, rect))
                        .collect();
                    painter.add(egui::Shape::line(pts, Stroke::new(3.5, col)));
                }
            }
            Some(ProjPick::ContextFace(_, hover)) => {
                for pts in self.context_face_edges(hover) {
                    let pts: Vec<Pos2> = pts
                        .iter()
                        .filter_map(|q| self.camera.project(q, rect))
                        .collect();
                    painter.add(egui::Shape::line(pts, Stroke::new(3.5, col)));
                }
            }
            Some(ProjPick::Context(_, pts)) => {
                let pts: Vec<Pos2> = pts
                    .iter()
                    .filter_map(|q| self.camera.project(q, rect))
                    .collect();
                painter.add(egui::Shape::line(pts, Stroke::new(3.5, col)));
            }
            Some(ProjPick::Edge(e)) => {
                let opts = crate::kernel::TessellationOptions::default();
                if let Ok(pts) = self.part().body.sample_edge(e, &opts) {
                    let pts: Vec<Pos2> = pts
                        .iter()
                        .filter_map(|q| self.camera.project(q, rect))
                        .collect();
                    painter.add(egui::Shape::line(pts, Stroke::new(3.5, col)));
                }
            }
            Some(ProjPick::Vertex(v)) => {
                let part = self.part();
                if let Ok(q) = crate::part::placement::point_of(&v, &part.body, &part.work)
                    && let Some(s) = self.camera.project(&q, rect)
                {
                    painter.circle_stroke(s, 7.0, Stroke::new(2.5, col));
                }
            }
            Some(ProjPick::Face(name)) => {
                let body = &self.part().body;
                let opts = crate::kernel::TessellationOptions::default();
                if let Some(f) = body.find_face(&name) {
                    for e in body.face_edges(f) {
                        if let Ok(pts) = body.sample_edge(e, &opts) {
                            let pts: Vec<Pos2> = pts
                                .iter()
                                .filter_map(|q| self.camera.project(q, rect))
                                .collect();
                            painter.add(egui::Shape::line(pts, Stroke::new(3.5, col)));
                        }
                    }
                }
            }
            Some(ProjPick::Work(id)) => {
                let part = self.part();
                if let Ok(shape) = crate::part::link::work_shape(
                    &part.work,
                    &id,
                    &part.sketches[i].sketch.plane,
                    &part.body,
                ) {
                    match shape {
                        crate::part::link::Shape3::Point(q) => {
                            if let Some(s) = self.camera.project(&q, rect) {
                                painter.circle_stroke(s, 7.0, Stroke::new(2.5, col));
                            }
                        }
                        crate::part::link::Shape3::Line(a, b) => {
                            if let (Some(a), Some(b)) =
                                (self.camera.project(&a, rect), self.camera.project(&b, rect))
                            {
                                painter.line_segment([a, b], Stroke::new(3.5, col));
                            }
                        }
                        _ => {}
                    }
                }
            }
            None => {}
        }
    }

    /// Colour of an earlier sketch drawn only so that Project Geometry can
    /// pick it (hidden or consumed).
    pub(super) fn project_source_color() -> Color32 {
        Color32::from_rgb(120, 150, 190)
    }
}

/// What a References row or the context menu of projected geometry does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RefAction {
    /// Keep the geometry, drop the link.
    BreakLink,
    /// Remove the projection and the constraints that used it.
    Delete,
    /// Pick a new source (Replace Reference).
    Replace,
    /// Select the projected curve.
    Select,
}

impl CadCraftApp {
    /// Applies `action` to the projection `target` of sketch `i` (one Undo
    /// step; on failure nothing changes).
    pub(super) fn reference_action(
        &mut self,
        i: usize,
        target: crate::part::RefTarget,
        action: RefAction,
    ) {
        use crate::part::RefTarget;
        match action {
            RefAction::Select => {
                if let RefTarget::Line(e) | RefTarget::Curve(e) = target {
                    self.tools.selected = vec![e];
                    self.tools.selected_constraints.clear();
                }
            }
            RefAction::Replace => match self.part().sketches[i].link_of(target) {
                Some(k) => {
                    self.relink = Some((i, k));
                    self.project_geometry = true;
                    self.file_status = None;
                }
                None => {
                    self.file_status = Some(Err(
                        "Replace Reference: only geometry projected from sketches or curved edges can be replaced yet"
                            .into(),
                    ));
                }
            },
            RefAction::BreakLink | RefAction::Delete => {
                let delete = action == RefAction::Delete;
                self.checkpoint(if delete {
                    "Delete Reference"
                } else {
                    "Break Link"
                });
                let ps = &mut self.part_mut().sketches[i];
                let r = if delete {
                    ps.delete_reference(target)
                } else {
                    ps.break_link(target)
                };
                match r {
                    Ok(()) => {
                        self.tools.selected.clear();
                        self.relink = None;
                        self.solve();
                    }
                    Err(e) => {
                        self.history.cancel();
                        let cmd = if delete { "Delete" } else { "Break Link" };
                        self.file_status = Some(Err(format!("{cmd}: {e}")));
                    }
                }
            }
        }
    }

    /// Context menu of projected geometry in the view: Break Link, Replace
    /// Reference, Delete.
    pub(super) fn reference_menu(
        &mut self,
        ui: &mut egui::Ui,
        i: usize,
        target: crate::part::RefTarget,
    ) {
        let ps = &self.part().sketches[i];
        let row = ps
            .references(&self.part().body, &self.part().work)
            .into_iter()
            .find(|r| r.target == target);
        let title = row
            .as_ref()
            .and_then(|r| r.label.clone())
            .unwrap_or_else(|| "Projected geometry".into());
        ui.label(egui::RichText::new(title).strong());
        if let Some(r) = &row {
            ui.label(egui::RichText::new(&r.source).weak());
            if !r.ok {
                ui.colored_label(
                    Color32::from_rgb(230, 140, 0),
                    "Warning: the source is missing (frozen)",
                );
            }
        }
        let linked = ps.link_of(target).is_some();
        for (label, action, tip) in [
            (
                "Break Link",
                RefAction::BreakLink,
                "Keep the geometry, no longer updated from its source",
            ),
            (
                "Replace Reference",
                RefAction::Replace,
                "Pick a new source; the constraints on it stay",
            ),
            (
                "Delete",
                RefAction::Delete,
                "Remove the projected geometry and the constraints that use it",
            ),
        ] {
            let enabled = action != RefAction::Replace || linked;
            if ui
                .add_enabled(enabled, egui::Button::new(label))
                .on_hover_text(tip)
                .on_disabled_hover_text(
                    "Projected model vertices and straight edges: Delete and project again",
                )
                .clicked()
            {
                self.reference_action(i, target, action);
                ui.close();
            }
        }
    }
}

/// The loop through edge `e` of the face beside it that is parallel to the
/// sketch (`normal`), else of a planar one beside it (Project Geometry with
/// Ctrl).
fn edge_loop_on(
    body: &crate::kernel::BRep,
    e: EdgeId,
    normal: &crate::kernel::UnitVector3,
) -> Vec<EdgeId> {
    let faces: Vec<_> = body
        .edge_faces(e)
        .into_iter()
        .filter(|f| body.face_plane(*f).is_some())
        .collect();
    let square = |f: &crate::kernel::FaceId| {
        body.face_plane(*f)
            .is_some_and(|pl| pl.normal.cross(normal).norm() < 1e-6)
    };
    let Some(f) = faces.iter().find(|f| square(f)).or(faces.first()).copied() else {
        return Vec::new();
    };
    body.face(f)
        .and_then(|face| {
            std::iter::once(&face.outer)
                .chain(&face.inner)
                .map(|l| l.coedges.iter().map(|c| c.edge).collect::<Vec<_>>())
                .find(|l| l.contains(&e))
        })
        .unwrap_or_default()
}
