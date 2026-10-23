//! The Fillet and Chamfer commands (`docs/spec/2.4-fillet.md`,
//! `2.5-chamfer.md`): pick edges in the view (a second click removes one),
//! set the size, see the result as a preview, OK / Apply.

use eframe::egui::{self, Color32, Rect, Shape, Stroke};

use super::CadCraftApp;
use crate::kernel::{EdgeId, Point3, TessellationOptions};
use crate::part::{BlendFeature, EdgeRef, FilletType, VariableRadius};

/// Inventor's Select Mode of the Fillet / Chamfer edge set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum EdgeMode {
    /// Edges one by one (with Automatic Edge Chain).
    #[default]
    Edge,
    /// A click on a face takes its loop nearest the click.
    Loop,
    /// A click on a face takes the edges where its feature meets others.
    Feature,
    /// All Edges / All Fillets / All Rounds of the body (dialog buttons).
    Solid,
}

/// The open Fillet / Chamfer command.
#[derive(Clone, Debug)]
pub(super) struct BlendDraft {
    /// The feature being defined (id and name are set on OK).
    pub def: BlendFeature,
    /// Automatic Edge Chain: a pick takes the tangentially connected edges.
    pub chain: bool,
    /// Edit Feature of this feature index (OK replaces it in place).
    pub edit: Option<usize>,
    /// The active edge set: 0 = `def.edges`, k = `def.sets[k − 1]`.
    pub set: usize,
    /// Select Mode.
    pub mode: EdgeMode,
}

impl BlendDraft {
    pub(super) fn fillet() -> Self {
        Self {
            def: BlendFeature::fillet(Vec::new(), 2.0),
            chain: true,
            edit: None,
            set: 0,
            mode: EdgeMode::Edge,
        }
    }

    pub(super) fn chamfer() -> Self {
        Self {
            def: BlendFeature::chamfer(Vec::new(), 1.0),
            chain: true,
            edit: None,
            set: 0,
            mode: EdgeMode::Edge,
        }
    }

    /// Edit Feature of blend `k`: its definition, edges and all.
    pub(super) fn editing(def: BlendFeature, k: usize) -> Self {
        Self {
            def,
            chain: true,
            edit: Some(k),
            set: 0,
            mode: EdgeMode::Edge,
        }
    }

    /// The active set's edges.
    pub(super) fn edges_mut(&mut self) -> &mut Vec<EdgeRef> {
        match self.set.checked_sub(1).filter(|&k| k < self.def.sets.len()) {
            Some(k) => &mut self.def.sets[k].edges,
            None => &mut self.def.edges,
        }
    }

    /// Whether faces (not edges) are picked: Loop, Feature, Full Round.
    pub(super) fn picks_faces(&self) -> bool {
        self.def.fillet_type == FilletType::FullRound
            || matches!(self.mode, EdgeMode::Loop | EdgeMode::Feature)
    }

    /// Adds `edges` (Loop / Feature / Solid) to the active set, skipping
    /// those already in any set.
    pub(super) fn add_edges(&mut self, body: &crate::kernel::BRep, edges: &[EdgeId]) {
        for &e in edges {
            if let Some(r) = crate::part::blend::edge_ref(body, e)
                && !self.contains(&r)
            {
                if self.def.fillet_type == FilletType::Variable {
                    self.add_variable(body, e, r);
                } else {
                    self.edges_mut().push(r);
                }
            }
        }
    }

    /// Variable: a straight edge with the current radius at both ends,
    /// starting at its first vertex.
    fn add_variable(&mut self, body: &crate::kernel::BRep, e: EdgeId, r: EdgeRef) {
        let straight = crate::part::select::edge_is_straight(body, e);
        let start = body
            .edge(e)
            .and_then(|x| body.vertex(x.start))
            .map(|v| [v.point.x, v.point.y, v.point.z]);
        let (true, Some(start)) = (straight, start) else {
            return;
        };
        let radius = match self.def.kind {
            crate::part::BlendKind::Fillet { radius } => radius,
            _ => 1.0,
        };
        self.def.variable.push(VariableRadius {
            edge: r,
            start,
            points: vec![[0.0, radius], [1.0, radius]],
        });
    }

    /// Picks model edge `e`: with Automatic Edge Chain its whole tangent
    /// chain; a pick of an edge already in the set removes its chain.
    pub(super) fn pick(&mut self, body: &crate::kernel::BRep, e: EdgeId) {
        if self.def.fillet_type == FilletType::Variable {
            // One straight edge per pick; a second pick removes it.
            let Some(r) = crate::part::blend::edge_ref(body, e) else {
                return;
            };
            let same = |a: &EdgeRef| {
                let (mut x, mut y) = (a.faces.clone(), r.faces.clone());
                x.sort();
                y.sort();
                x == y
            };
            match self.def.variable.iter().position(|v| same(&v.edge)) {
                Some(k) => {
                    self.def.variable.remove(k);
                }
                None => self.add_variable(body, e, r),
            }
            return;
        }
        let refs = if self.chain {
            crate::part::blend::tangent_chain(body, e)
        } else {
            crate::part::blend::edge_ref(body, e).into_iter().collect()
        };
        let Some(first) = refs.first().cloned() else {
            return;
        };
        let present = self.contains(&first);
        for r in refs {
            if self.contains(&r) == present {
                self.toggle(r);
            }
        }
    }

    pub(super) fn contains(&self, r: &EdgeRef) -> bool {
        self.def.all_edges().iter().any(|a| {
            let mut x = a.faces.clone();
            let mut y = r.faces.clone();
            x.sort();
            y.sort();
            x == y
        })
    }

    /// Adds the edge, or removes it when it is already in the set.
    pub(super) fn toggle(&mut self, r: EdgeRef) {
        let same = |a: &EdgeRef| {
            let mut x = a.faces.clone();
            let mut y = r.faces.clone();
            x.sort();
            y.sort();
            x == y
        };
        let edges = self.edges_mut();
        if let Some(k) = edges.iter().position(same) {
            edges.remove(k);
        } else if !self.contains(&r) {
            self.edges_mut().push(r);
        }
    }
}

impl CadCraftApp {
    /// Starts Fillet (`chamfer` false) or Chamfer, with `edge` already in
    /// the set (from the edge's suggestion bar).
    pub(super) fn start_blend(&mut self, chamfer: bool, edge: Option<EdgeId>) {
        self.finish_sketch();
        self.mode = super::Mode::Normal;
        self.cancel_extrude();
        self.work_draft = None;
        let mut d = if chamfer {
            BlendDraft::chamfer()
        } else {
            BlendDraft::fillet()
        };
        self.preset_start(d.def.command(), &mut d.def);
        if let Some(e) = edge {
            d.pick(&self.part().body, e);
        }
        // Edges selected beforehand (Ctrl/Shift, box, Select Tangencies).
        for x in std::mem::take(&mut self.sel) {
            if let super::select::SelItem::Edge(e) = x
                && Some(e) != edge
                && let Some(r) = crate::part::blend::edge_ref(&self.part().body, e)
                && !d.contains(&r)
            {
                d.toggle(r);
            }
        }
        self.face_sel = None;
        self.edge_sel = None;
        self.blend_draft = Some(d);
    }

    /// Loop / Feature / Full Round: a click on face `name` at `at` (screen).
    pub(super) fn blend_face_pick(&mut self, name: &str, at: egui::Pos2, rect: Rect) {
        let body = self.part().body.clone();
        let Some(f) = body.find_face(name) else {
            return;
        };
        let Some(d) = self.blend_draft.as_ref() else {
            return;
        };
        if d.def.fillet_type == FilletType::FullRound {
            if let Some(d) = self.blend_draft.as_mut() {
                d.def.full_round = Some(name.to_owned());
            }
            return;
        }
        let edges = match d.mode {
            EdgeMode::Loop => {
                // The clicked point on the face: the loop nearest to it.
                let Some((o, dir)) = self.camera.ray(at, rect) else {
                    return;
                };
                let Some(dir) = dir.try_normalize(1e-12) else {
                    return;
                };
                let Some((_, t)) = self.picker().pick(&o, &dir) else {
                    return;
                };
                crate::part::select::loop_edges(&body, f, o + dir * t)
            }
            EdgeMode::Feature => match crate::part::select::feature_id_of_face(name) {
                Some(id) => crate::part::select::feature_edges(&body, id),
                None => Vec::new(),
            },
            _ => Vec::new(),
        };
        if let Some(d) = self.blend_draft.as_mut() {
            d.add_edges(&body, &edges);
        }
    }

    /// Select Mode Solid: All Edges / All Fillets / All Rounds of every body.
    pub(super) fn blend_solid_pick(&mut self, which: crate::part::select::SolidEdges) {
        let body = self.part().body.clone();
        let edges = crate::part::select::solid_edges(&body, None, which);
        if let Some(d) = self.blend_draft.as_mut() {
            d.add_edges(&body, &edges);
        }
    }

    /// Model edges of the current body that an edge reference stands for
    /// (for highlighting the set): the edges between the two named faces
    /// nearest to its hint, or all of them for a circular edge.
    pub(super) fn edges_of_ref(&self, r: &EdgeRef) -> Vec<EdgeId> {
        let body = &self.part().body;
        let (m0, m1) = (
            crate::part::blend::face_matcher(body, &r.faces[0]),
            crate::part::blend::face_matcher(body, &r.faces[1]),
        );
        let name = |f: crate::kernel::FaceId| body.face(f).and_then(|x| x.name.as_deref());
        let picker = self.picker();
        let shared: Vec<EdgeId> = body
            .edges()
            .map(|(e, _)| e)
            .filter(|&e| {
                let fs = picker.faces_of(e);
                fs.iter().any(|&f| m0(name(f))) && fs.iter().any(|&f| m1(name(f)))
            })
            .collect();
        let straight: Vec<EdgeId> = shared
            .iter()
            .copied()
            .filter(|&e| body.edge_direction(e).is_some())
            .collect();
        // Straight edges between planar faces: those on the line through
        // the hint (one run); otherwise (circles, facets) all of them.
        let hint = Point3::from(r.hint);
        let on_line: Vec<EdgeId> = straight
            .iter()
            .copied()
            .filter(|&e| {
                let Some(x) = body.edge(e) else { return false };
                let (Some(a), Some(b)) = (body.vertex(x.start), body.vertex(x.end)) else {
                    return false;
                };
                let d = (b.point - a.point).normalize();
                let off = hint - a.point;
                (off - d * off.dot(&d)).norm() < 1e-3
            })
            .collect();
        // A curved (faceted) face: the whole circle.
        let curved = shared.iter().any(|&e| {
            picker
                .faces_of(e)
                .into_iter()
                .any(|f| body.face(f).is_some_and(|x| x.facet_of.is_some()))
        });
        if on_line.is_empty() || curved {
            shared
        } else {
            on_line
        }
    }

    /// The preview of the open command: edges of the result's new faces,
    /// or why it cannot be built (cached by the definition).
    pub(super) fn blend_preview(&self) -> Result<Vec<Vec<Point3>>, String> {
        let Some(d) = &self.blend_draft else {
            return Err(String::new());
        };
        if !d.def.has_input() {
            return Err(if d.def.fillet_type == FilletType::FullRound {
                "select the middle face".into()
            } else {
                "select edges".into()
            });
        }
        let key = format!("blend|{:?}|{:?}", d.edit, d.def);
        // Cached or still computing: no copy of the part this frame.
        if let Some(r) = self.preview_ready(&key) {
            return r;
        }
        let mut trial = self.part().clone();
        let (def, edit) = (d.def.clone(), d.edit);
        self.preview_async(key, move || {
            let built = match edit {
                Some(k) => trial.replace_feature(k, crate::part::Feature::Blend(def)),
                None => trial.add_blend(def),
            };
            built
                .map(|id| Self::feature_polylines(&trial, &id))
                .map_err(|e| e.to_string())
        })
    }

    /// Draws the picked edges (blue) and the one under the cursor (green).
    pub(super) fn draw_blend_edges(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        hover: Option<EdgeId>,
        face_hover: Option<&str>,
    ) {
        let Some(d) = &self.blend_draft else { return };
        let body = &self.part().body;
        let opts = TessellationOptions::default();
        let line = |e: EdgeId, color: Color32| {
            if let Ok(pts) = body.sample_edge(e, &opts) {
                self.draw_visible_polyline(painter, rect, &pts, Stroke::new(4.0, color));
            }
        };
        // The active set blue, the others grey-blue.
        let active: Vec<EdgeRef> = match d.set.checked_sub(1) {
            Some(k) => d
                .def
                .sets
                .get(k)
                .map(|s| s.edges.clone())
                .unwrap_or_default(),
            None => d.def.edges.clone(),
        };
        for r in d.def.all_edges() {
            let col = if active.contains(&r) || d.def.fillet_type == FilletType::Variable {
                Color32::from_rgb(40, 110, 230)
            } else {
                Color32::from_rgb(110, 140, 190)
            };
            for e in self.edges_of_ref(&r) {
                line(e, col);
            }
        }
        // Full Round: the middle face's outline.
        if let Some(name) = &d.def.full_round
            && let Some(f) = body.find_face(name)
        {
            for e in crate::part::select::edges_of_face(body, f) {
                line(e, Color32::from_rgb(40, 110, 230));
            }
        }
        if let Some(e) = hover {
            line(e, Color32::from_rgb(60, 190, 90));
        }
        // A face under the cursor (Loop / Feature / Full Round): its outline.
        if let Some(f) = face_hover.and_then(|n| body.find_face(n)) {
            for e in crate::part::select::edges_of_face(body, f) {
                line(e, Color32::from_rgb(60, 190, 90));
            }
        }
        if let Ok(edges) = self.blend_preview() {
            let stroke = Stroke::new(2.0, Color32::from_rgb(230, 120, 20));
            for e in edges {
                let pts: Vec<_> = e
                    .iter()
                    .filter_map(|p| self.camera.project(p, rect))
                    .collect();
                painter.add(Shape::line(pts, stroke));
            }
        }
    }

    /// OK (`keep_open` false) or Apply: adds the feature.
    pub(super) fn commit_blend(&mut self, keep_open: bool) {
        let Some(mut d) = self.blend_draft.clone() else {
            return;
        };
        let cmd = if d.edit.is_some() {
            "Edit Feature"
        } else {
            d.def.command()
        };
        self.preset_stamp(d.def.command(), &mut d.def);
        self.checkpoint(cmd);
        let built = match d.edit {
            Some(k) => self
                .part_mut()
                .replace_feature(k, crate::part::Feature::Blend(d.def.clone())),
            None => self.part_mut().add_blend(d.def.clone()),
        };
        if built.is_err()
            && let Some(k) = d.edit
        {
            // Stay in the edit: rolled back again.
            self.part_mut().rollback = Some(k);
            let _ = self.part_mut().regenerate();
        }
        if built.is_ok() {
            self.file_status = None;
            self.preset_used(d.def.command(), &d.def, d.edit.is_none());
        }
        match built {
            Ok(_) if d.edit.is_some() => {
                self.scene_dirty = true;
                self.blend_draft = None;
                self.selection = super::Selection::Feature(d.edit.unwrap_or_default());
            }
            Ok(_) => {
                self.scene_dirty = true;
                if keep_open {
                    let mut next = d;
                    next.def.edges.clear();
                    next.def.sets.clear();
                    next.def.variable.clear();
                    next.def.full_round = None;
                    next.set = 0;
                    self.blend_draft = Some(next);
                } else {
                    self.blend_draft = None;
                    self.selection = super::Selection::Feature(self.part().last_inserted());
                }
            }
            Err(e) => {
                self.history.cancel();
                self.file_status = Some(Err(format!("{cmd}: {e}")));
            }
        }
    }
}

/// The direction on face `name` square to the edge (unit tangent `t`) at
/// `p`, pointing into the face, and the face's outward normal there (from
/// its display mesh: the nearest vertex, the side its vertices lie on).
fn into_face(
    body: &crate::kernel::BRep,
    name: &str,
    p: Point3,
) -> Option<(crate::kernel::Vector3, Vec<Point3>)> {
    let mut best: Option<(f64, crate::kernel::Vector3)> = None;
    let mut pts = Vec::new();
    for (_, f) in body
        .faces()
        .filter(|(_, f)| f.name.as_deref() == Some(name))
    {
        let Some(m) = &f.mesh else { continue };
        for (q, n) in m.positions.iter().zip(&m.normals) {
            let d = (q - p).norm();
            if best.as_ref().is_none_or(|b| d < b.0) {
                best = Some((d, *n));
            }
            pts.push(*q);
        }
    }
    best.map(|(_, n)| (n, pts))
}

impl CadCraftApp {
    /// The arrows of the open Fillet / Chamfer (`handles_for`); a drag sets
    /// the value (`ManipOwner::Blend`).
    pub(super) fn blend_handles(&self) -> Vec<crate::gui::manip::Handle> {
        match &self.blend_draft {
            Some(d) => handles_for(&self.part().body, d),
            None => Vec::new(),
        }
    }
}

/// On the first edge of the active set, along the face(s) square to the
/// edge, as long as the radius / distance(s).
fn handles_for(body: &crate::kernel::BRep, d: &BlendDraft) -> Vec<crate::gui::manip::Handle> {
    use crate::gui::manip::{DragGeom, Handle};
    use crate::part::{BlendKind, ChamferMethod};
    let edges = if d.set == 0 {
        &d.def.edges
    } else {
        match d.def.sets.get(d.set - 1) {
            Some(s) => &s.edges,
            None => return Vec::new(),
        }
    };
    let Some(e) = edges.first() else {
        return Vec::new();
    };
    let p = Point3::from(e.hint);
    let (Some((na, pa)), Some((nb, pb))) = (
        into_face(body, &e.faces[0], p),
        into_face(body, &e.faces[1], p),
    ) else {
        return Vec::new();
    };
    let t = na.cross(&nb);
    if t.norm() < 1e-6 {
        return Vec::new(); // tangent faces: no corner to size
    }
    // On face `n` (its points `pts`): square to the edge, into the face.
    let along = |n: &crate::kernel::Vector3, pts: &[Point3]| {
        let mut dir = t.cross(n).normalize();
        let side: f64 = pts.iter().map(|q| (q - p).dot(&dir)).sum();
        if side < 0.0 {
            dir = -dir;
        }
        dir
    };
    let arrow = |field: &'static str, dir: crate::kernel::Vector3, v: f64| Handle {
        field,
        path: vec![p, p + dir * v],
        grip: p + dir * v,
        geom: DragGeom::Linear {
            origin: p,
            dir,
            scale: 1.0,
        },
        arrow: Some(dir),
        angle: false,
        signed: false,
    };
    let (da, db) = (along(&na, &pa), along(&nb, &pb));
    match &d.def.kind {
        BlendKind::Fillet { radius } => {
            if matches!(
                d.def.fillet_type,
                FilletType::Variable | FilletType::FullRound
            ) {
                return Vec::new();
            }
            let (field, r) = if d.set == 0 {
                ("radius", *radius)
            } else {
                let Some(s) = d.def.sets.get(d.set - 1) else {
                    return Vec::new();
                };
                let Some(f) = crate::part::params::SET_RADII.get(d.set - 1) else {
                    return Vec::new();
                };
                (*f, s.radius)
            };
            vec![arrow(field, da, r)]
        }
        BlendKind::Chamfer {
            method,
            distance,
            distance2,
            flip,
            ..
        } => {
            let (first, second) = if *flip { (db, da) } else { (da, db) };
            match method {
                ChamferMethod::Distance => {
                    vec![arrow("distance", da, *distance)]
                }
                ChamferMethod::TwoDistances => vec![
                    arrow("distance", first, *distance),
                    arrow("distance2", second, *distance2),
                ],
                ChamferMethod::DistanceAngle => vec![arrow("distance", first, *distance)],
            }
        }
    }
}

#[cfg(test)]
mod handle_tests {
    use super::*;
    use crate::part::PartDocument;

    /// The radius arrow of a box edge lies on one of its faces, square to
    /// the edge, as long as the radius; a chamfer with two distances has
    /// one arrow on each face.
    #[test]
    fn arrows_of_a_box_edge() {
        let part = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
        // Top / Front edge: y = 0, z = 20, along x.
        let e = EdgeRef {
            faces: ["Box1:Top".into(), "Box1:Front".into()],
            hint: [20.0, 0.0, 20.0],
        };
        let mut d = BlendDraft::fillet();
        d.def.edges.push(e.clone());
        if let crate::part::BlendKind::Fillet { radius } = &mut d.def.kind {
            *radius = 3.0;
        }
        let hs = handles_for(&part.body, &d);
        assert_eq!(hs.len(), 1);
        let h = &hs[0];
        assert_eq!(h.field, "radius");
        let tip = h.grip;
        // On the top face (z = 20), 3 mm into it (y = 3).
        assert!(
            (tip.z - 20.0).abs() < 1e-6 && (tip.y - 3.0).abs() < 1e-6,
            "{tip:?}"
        );
        let mut c = BlendDraft::chamfer();
        c.def.edges.push(e);
        if let crate::part::BlendKind::Chamfer {
            method,
            distance,
            distance2,
            ..
        } = &mut c.def.kind
        {
            *method = crate::part::ChamferMethod::TwoDistances;
            *distance = 2.0;
            *distance2 = 4.0;
        }
        let hs = handles_for(&part.body, &c);
        assert_eq!(hs.len(), 2);
        let (a, b) = (hs[0].grip, hs[1].grip);
        assert!(
            (a.y - 2.0).abs() < 1e-6 && (a.z - 20.0).abs() < 1e-6,
            "{a:?}"
        );
        assert!((b.z - 16.0).abs() < 1e-6 && b.y.abs() < 1e-6, "{b:?}");
    }
}
