//! The Rectangular Pattern, Circular Pattern and Mirror commands of the
//! part (`docs/spec/2.11-patterns.md`, `docs/spec/2.12-mirror.md`): pick the
//! features (a face in the view selects its feature, a row in the browser
//! the feature itself) or the solids, then the direction(s), the axis or
//! the mirror plane (the selector advances by itself); set count, spacing
//! or angle; preview, OK / Apply. (`pattern_dialog.rs` is the 2D sketch
//! pattern.)

use eframe::egui::{self, Color32, Rect, Stroke};

use super::CadCraftApp;
use crate::kernel::{EdgeId, FaceId, Point3};
use crate::part::{
    Feature, LineRef, PatternDirection, PatternFeature, PatternKind, PlaneRef, PullRef,
};

/// The selector that takes the next click.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PatternInput {
    /// Features (or Solids in solid mode).
    Geometry,
    /// Rectangular: Direction 1.
    Dir1,
    /// Rectangular: Direction 2.
    Dir2,
    /// Circular: Rotation Axis.
    Axis,
    /// Mirror: Mirror Plane.
    Plane,
    /// Sketch Driven: the sketch whose centre points place the copies.
    Sketch,
    /// Sketch Driven: the Base Point (optional).
    Base,
}

/// The open pattern / mirror command.
#[derive(Clone, Debug)]
pub(super) struct PatternDraft {
    /// The feature being defined (id and name are set on OK).
    pub def: PatternFeature,
    /// Edit Feature of this feature index.
    pub edit: Option<usize>,
    /// The active selector.
    pub input: PatternInput,
}

impl PatternDraft {
    pub(super) fn new(def: PatternFeature) -> Self {
        Self {
            def,
            edit: None,
            input: PatternInput::Geometry,
        }
    }

    pub(super) fn editing(def: PatternFeature, k: usize) -> Self {
        let mut d = Self::new(def);
        d.edit = Some(k);
        d.input = d.placement_input();
        d
    }

    /// The selector after the geometry: Direction 1, the axis or the plane.
    pub(super) fn placement_input(&self) -> PatternInput {
        match self.def.kind {
            PatternKind::Rectangular { .. } => PatternInput::Dir1,
            PatternKind::Circular { .. } => PatternInput::Axis,
            PatternKind::Mirror { .. } => PatternInput::Plane,
            PatternKind::Sketch { .. } => PatternInput::Sketch,
        }
    }

    /// Whether the active selector takes curves as a path (any model edge,
    /// sketch curves): the rectangular directions.
    pub(super) fn picks_paths(&self) -> bool {
        matches!(self.input, PatternInput::Dir1 | PatternInput::Dir2)
    }

    /// Whether the active selector takes a sketch.
    pub(super) fn picks_sketch(&self) -> bool {
        self.input == PatternInput::Sketch
    }

    /// Whether the active selector takes a point (vertex, work point).
    pub(super) fn picks_points(&self) -> bool {
        self.input == PatternInput::Base
    }

    /// Sets the active direction's path (a curved edge or sketch curves);
    /// a sketch curve of the same sketch is added to the path (or dropped
    /// when it is in it already).
    pub(super) fn set_path(&mut self, path: crate::part::PathRef) -> bool {
        use crate::part::PathRef;
        let input = self.input;
        let PatternKind::Rectangular { first, second } = &mut self.def.kind else {
            return false;
        };
        let d = match input {
            PatternInput::Dir1 => first,
            PatternInput::Dir2 => second.get_or_insert_with(PatternDirection::new),
            _ => return false,
        };
        let merged = match (d.path.take(), path) {
            (
                Some(PathRef::Sketch { sketch, mut curves }),
                PathRef::Sketch {
                    sketch: s2,
                    curves: c2,
                },
            ) if sketch == s2 => {
                for c in c2 {
                    if let Some(i) = curves.iter().position(|x| *x == c) {
                        curves.remove(i);
                    } else {
                        curves.push(c);
                    }
                }
                PathRef::Sketch { sketch, curves }
            }
            (_, p) => p,
        };
        d.path = (!merged.is_empty()).then_some(merged);
        d.dir = None;
        true
    }

    /// Sketch Driven: sets the sketch (by id).
    pub(super) fn set_sketch(&mut self, id: String) -> bool {
        let PatternKind::Sketch { sketch, .. } = &mut self.def.kind else {
            return false;
        };
        *sketch = Some(id);
        true
    }

    /// Sketch Driven: sets the Base Point.
    pub(super) fn set_base(&mut self, p: crate::part::PointRef) -> bool {
        let PatternKind::Sketch { base, .. } = &mut self.def.kind else {
            return false;
        };
        *base = Some(p);
        true
    }

    /// Whether the active selector takes features / solids (faces).
    pub(super) fn picks_geometry(&self) -> bool {
        self.input == PatternInput::Geometry
    }

    /// Whether the active selector takes straight edges.
    pub(super) fn picks_lines(&self) -> bool {
        matches!(
            self.input,
            PatternInput::Dir1 | PatternInput::Dir2 | PatternInput::Axis
        )
    }

    /// Whether the active selector takes planes (faces, origin and work
    /// planes).
    pub(super) fn picks_planes(&self) -> bool {
        matches!(
            self.input,
            PatternInput::Dir1 | PatternInput::Dir2 | PatternInput::Plane
        )
    }

    /// Adds feature `id` (or removes it when it is in the list already).
    pub(super) fn toggle_feature(&mut self, id: &str) {
        if let Some(i) = self.def.features.iter().position(|f| f == id) {
            self.def.features.remove(i);
        } else {
            self.def.features.push(id.to_owned());
            if self.def.features.len() == 1 {
                self.input = self.placement_input();
            }
        }
    }

    /// Adds solid `name` (or removes it).
    pub(super) fn toggle_solid(&mut self, name: &str) {
        if let Some(i) = self.def.solids.iter().position(|f| f == name) {
            self.def.solids.remove(i);
        } else {
            self.def.solids.push(name.to_owned());
            if self.def.solids.len() == 1 {
                self.input = self.placement_input();
            }
        }
    }

    /// Sets the active direction / axis / plane from a pick.
    pub(super) fn set_ref(&mut self, line: Option<LineRef>, plane: Option<PlaneRef>) -> bool {
        let pull = match (&line, &plane) {
            (Some(l), _) => Some(PullRef::Line(l.clone())),
            (None, Some(p)) => Some(PullRef::Plane(p.clone())),
            _ => None,
        };
        let input = self.input;
        match (&mut self.def.kind, input) {
            (PatternKind::Rectangular { first, .. }, PatternInput::Dir1) => {
                let Some(p) = pull else { return false };
                // The selector stays: another click replaces the direction
                // (Direction 2 is chosen in its own section).
                first.dir = Some(p);
                first.path = None;
                true
            }
            (PatternKind::Rectangular { second, .. }, PatternInput::Dir2) => {
                let Some(p) = pull else { return false };
                let s = second.get_or_insert_with(PatternDirection::new);
                s.dir = Some(p);
                s.path = None;
                true
            }
            (PatternKind::Circular { axis, .. }, PatternInput::Axis) => {
                let Some(l) = line else { return false };
                *axis = Some(l);
                true
            }
            (PatternKind::Mirror { plane: pl }, PatternInput::Plane) => {
                let Some(p) = plane else { return false };
                *pl = Some(p);
                true
            }
            _ => false,
        }
    }
}

impl CadCraftApp {
    /// Opens the command; a feature selected in the browser (or by a face)
    /// becomes the first input, as in Inventor.
    pub(super) fn start_pattern(&mut self, def: PatternFeature) {
        let pre = match self.selection {
            super::Selection::Feature(k) => self.part().features.get(k).map(|f| f.id().to_owned()),
            _ => None,
        };
        // Sketch Driven: a selected sketch is its Sketch input.
        let pre_sketch = match self.selection {
            super::Selection::Sketch(i) => self.part().sketches.get(i).map(|s| s.id.clone()),
            _ => None,
        };
        self.finish_sketch();
        self.mode = super::Mode::Normal;
        self.cancel_extrude();
        self.work_draft = None;
        self.blend_draft = None;
        self.sel.clear();
        self.face_sel = None;
        self.edge_sel = None;
        let mut d = PatternDraft::new(def);
        if let Some(id) = pre {
            d.toggle_feature(&id);
        }
        if let Some(id) = pre_sketch {
            d.set_sketch(id);
        }
        self.pattern_draft = Some(d);
    }

    /// A click on a face while the Features / Solids selector is active.
    pub(super) fn pattern_pick_face(&mut self, f: FaceId) {
        let body = &self.part().body;
        let (Some(name), solid) = (
            body.face(f).and_then(|x| x.name.clone()),
            crate::part::select::body_of_face(body, f),
        ) else {
            return;
        };
        let feature = crate::part::select::feature_id_of_face(&name).map(str::to_owned);
        let Some(d) = self.pattern_draft.as_mut() else {
            return;
        };
        if d.def.solid_mode {
            if let Some(s) = solid {
                d.toggle_solid(&s);
            }
        } else if let Some(id) = feature
            && id != d.def.id
        {
            d.toggle_feature(&id);
        }
    }

    /// A click in the browser on feature `k` while the Features selector is
    /// active: it is taken (or dropped). Returns whether it was used.
    pub(super) fn pattern_pick_browser(&mut self, k: usize) -> bool {
        let Some(id) = self.part().features.get(k).map(|f| f.id().to_owned()) else {
            return false;
        };
        let Some(d) = self.pattern_draft.as_mut() else {
            return false;
        };
        if d.def.solid_mode || d.edit.is_some_and(|e| k >= e) {
            return false;
        }
        if !d.picks_geometry() {
            d.input = PatternInput::Geometry;
        }
        d.toggle_feature(&id);
        true
    }

    /// A click on a straight edge or a plane while a direction / axis /
    /// plane selector is active.
    pub(super) fn pattern_pick_ref(&mut self, edge: Option<EdgeId>, plane: Option<PlaneRef>) {
        let body = &self.part().body;
        // A curved edge of a rectangular direction: the copies follow it.
        let path = edge
            .filter(|&e| body.edge_direction(e).is_none())
            .and_then(|e| crate::part::blend::edge_ref(body, e))
            .map(|r| crate::part::PathRef::Edges(vec![r]));
        let line = edge
            .filter(|&e| body.edge_direction(e).is_some())
            .and_then(|e| body.edge(e).and_then(|x| x.name.clone()))
            .map(LineRef::Edge);
        if let Some(d) = self.pattern_draft.as_mut() {
            match path {
                Some(p) if d.picks_paths() => {
                    d.set_path(p);
                }
                _ => {
                    d.set_ref(line, plane);
                }
            }
        }
    }

    /// A click that is no face, edge or plane: a sketch curve as a path, a
    /// sketch for Sketch Driven, a vertex or work point as its Base Point.
    pub(super) fn pattern_pick_other(&mut self, p: egui::Pos2, rect: Rect) {
        let Some(d) = self.pattern_draft.as_ref() else {
            return;
        };
        if d.picks_paths() {
            let hit = self.part().sketches.iter().find_map(|ps| {
                if !ps.visible {
                    return None;
                }
                let (e, _) = super::overlay::pick_entity(
                    &self.camera,
                    rect,
                    &ps.sketch,
                    p,
                    super::overlay::CURVE_SNAP_PX,
                )?;
                Some((ps.id.clone(), e))
            });
            if let Some((sketch, e)) = hit
                && let Some(d) = self.pattern_draft.as_mut()
            {
                d.set_path(crate::part::PathRef::Sketch {
                    sketch,
                    curves: vec![e],
                });
            }
        } else if d.picks_sketch() {
            let id = self
                .sketch_under(p, rect)
                .and_then(|i| self.part().sketches.get(i))
                .map(|ps| ps.id.clone());
            if let Some(id) = id
                && let Some(d) = self.pattern_draft.as_mut()
                && d.set_sketch(id)
            {
                d.input = PatternInput::Base;
            }
        } else if d.picks_points() {
            let at = match self.work_axis_or_point_under(p, rect) {
                Some(super::work::WpInput::Point(r)) => Some(r),
                _ => self.vertex_under(p, rect),
            };
            if let Some(r) = at
                && let Some(d) = self.pattern_draft.as_mut()
            {
                d.set_base(r);
            }
        }
    }

    /// Sketch Driven: picks a sketch from the browser while its selector is
    /// active. Returns whether it was used.
    pub(super) fn pattern_pick_sketch_row(&mut self, index: usize) -> bool {
        let Some(id) = self.part().sketches.get(index).map(|s| s.id.clone()) else {
            return false;
        };
        match self.pattern_draft.as_mut() {
            Some(d) if d.picks_sketch() => {
                let set = d.set_sketch(id);
                if set {
                    d.input = PatternInput::Base;
                }
                set
            }
            _ => false,
        }
    }

    /// The faces to highlight: those of the chosen features (copies of an
    /// earlier pattern included) or of the chosen solids.
    pub(super) fn pattern_faces(&self) -> Vec<FaceId> {
        let Some(d) = &self.pattern_draft else {
            return Vec::new();
        };
        let body = &self.part().body;
        let mut v = Vec::new();
        if d.def.solid_mode {
            for s in &d.def.solids {
                v.extend(crate::part::select::faces_of_body(body, s));
            }
        } else {
            for id in &d.def.features {
                v.extend(crate::part::select::faces_of_feature(body, id));
            }
        }
        // A mirror plane on a face shows too.
        if let PatternKind::Mirror {
            plane: Some(PlaneRef::Face(n)),
        } = &d.def.kind
            && let Some(f) = body.find_face(n)
        {
            v.push(f);
        }
        v
    }

    pub(super) fn pattern_preview(&self) -> Result<Vec<Vec<Point3>>, String> {
        let Some(d) = &self.pattern_draft else {
            return Err(String::new());
        };
        let key = format!("pattern|{:?}|{:?}", d.edit, d.def);
        // Cached or still computing: no copy of the part this frame.
        if let Some(r) = self.preview_ready(&key) {
            return r;
        }
        let mut trial = self.part().clone();
        let (def, edit) = (d.def.clone(), d.edit);
        // OK reuses what the preview built (see `take_preview_part`).
        let (slot, sig, k2) = (
            self.preview_part.clone(),
            self.part_signature(),
            key.clone(),
        );
        self.preview_async(key, move || {
            let built = match edit {
                Some(k) => trial.replace_feature(k, Feature::Pattern(def)),
                None => trial.add_pattern(def),
            };
            let r = built
                .map(|id| Self::feature_polylines(&trial, &id))
                .map_err(|e| e.to_string());
            if r.is_ok()
                && let Ok(mut s) = slot.lock()
            {
                *s = Some((k2, sig, trial));
            }
            r
        })
    }

    /// Arrows for the directions, the axis as a long line, from the middle
    /// of the chosen geometry (or of the body).
    pub(super) fn draw_pattern_refs(&self, painter: &egui::Painter, rect: Rect) {
        let Some(d) = &self.pattern_draft else { return };
        let part = self.part();
        let body = &part.body;
        // The kernel's stored triangles (no tessellation in the frame).
        let mut pts: Vec<Point3> = self
            .pattern_faces()
            .into_iter()
            .filter_map(|f| body.face(f).and_then(|x| x.mesh.as_ref()))
            .flat_map(|m| m.positions.iter().copied())
            .collect();
        if pts.is_empty() {
            pts = body.vertices().map(|(_, v)| v.point).collect();
        }
        if pts.is_empty() {
            return;
        }
        let c = pts
            .iter()
            .fold(Point3::origin(), |a, p| a + p.coords / pts.len() as f64);
        let size = body
            .vertices()
            .map(|(_, v)| (v.point - c).norm())
            .fold(10.0, f64::max);
        let arrow = |from: Point3, dir: crate::kernel::Vector3, col: Color32| {
            if let (Some(a), Some(b)) = (
                self.camera.project(&from, rect),
                self.camera.project(&(from + dir * size * 0.5), rect),
            ) {
                painter.arrow(a, b - a, Stroke::new(2.5, col));
            }
        };
        let resolve = |p: &PullRef, flip: bool| {
            let v = match p {
                PullRef::Plane(pl) => crate::part::placement::plane_of(pl, body, &part.work)
                    .ok()
                    .map(|x| x.normal.into_inner()),
                PullRef::Line(l) => l.resolve(body, &part.work).ok().map(|x| x.1),
            }?;
            Some(if flip { -v } else { v })
        };
        let orange = Color32::from_rgb(230, 120, 20);
        let blue = Color32::from_rgb(40, 120, 220);
        // A path: drawn along its curve.
        let path_line = |dir: &PatternDirection, col: Color32| {
            let Some(path) = dir.path.as_ref() else {
                return;
            };
            let Ok(g) = path.resolve(&part.sketches, body, c) else {
                return;
            };
            let pts: Vec<egui::Pos2> = g
                .polyline()
                .iter()
                .filter_map(|q| self.camera.project(q, rect))
                .collect();
            painter.add(egui::Shape::line(pts, Stroke::new(3.0, col)));
        };
        match &d.def.kind {
            PatternKind::Rectangular { first, second } => {
                path_line(first, orange);
                if let Some(v) = first.dir.as_ref().and_then(|p| resolve(p, first.flip)) {
                    arrow(c, v, orange);
                }
                if let Some(s) = second {
                    path_line(s, blue);
                    if let Some(v) = s.dir.as_ref().and_then(|p| resolve(p, s.flip)) {
                        arrow(c, v, blue);
                    }
                }
            }
            PatternKind::Sketch {
                sketch: Some(id),
                base,
            } => {
                // The centre points marked; the base point as a ring.
                if let Some(ps) = part.sketches.iter().find(|s| &s.id == id) {
                    let pl = &ps.sketch.plane;
                    for q in crate::part::pattern::sketch_centres(&ps.sketch) {
                        if let Some(s) = self.camera.project(&pl.point_at(q.x, q.y), rect) {
                            painter.circle_filled(s, 4.0, orange);
                        }
                    }
                }
                if let Some(b) = base
                    && let Ok(q) = crate::part::placement::point_of(b, body, &part.work)
                    && let Some(s) = self.camera.project(&q, rect)
                {
                    painter.circle_stroke(s, 7.0, Stroke::new(2.0, blue));
                }
            }
            PatternKind::Circular {
                axis: Some(l),
                flip,
                ..
            } => {
                if let Ok((o, a)) = l.resolve(body, &part.work) {
                    let a = if *flip { -a } else { a };
                    let foot = o + a * (c - o).dot(&a);
                    if let (Some(p), Some(q)) = (
                        self.camera.project(&(foot - a * size), rect),
                        self.camera.project(&(foot + a * size), rect),
                    ) {
                        painter.line_segment([p, q], Stroke::new(2.0, orange));
                    }
                    arrow(foot, a, orange);
                }
            }
            _ => {}
        }
    }

    /// OK (`keep_open` false) or Apply.
    pub(super) fn commit_pattern(&mut self, keep_open: bool) {
        let Some(d) = self.pattern_draft.clone() else {
            return;
        };
        let cmd = if d.edit.is_some() {
            "Edit Feature"
        } else {
            d.def.command()
        };
        let key = format!("pattern|{:?}|{:?}", d.edit, d.def);
        let ready = self.take_preview_part(&key);
        self.checkpoint(cmd);
        let built = match (ready, d.edit) {
            // The preview built exactly this from the part as it is: take it.
            (Some(p), _) => {
                let id = p
                    .features
                    .get(d.edit.unwrap_or(p.last_inserted()))
                    .map(|f| f.id().to_owned())
                    .unwrap_or_default();
                *self.part_mut() = p;
                Ok(id)
            }
            (None, Some(k)) => self
                .part_mut()
                .replace_feature(k, Feature::Pattern(d.def.clone())),
            (None, None) => self.part_mut().add_pattern(d.def.clone()),
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
                if keep_open && d.edit.is_none() {
                    // Apply: the same placement, new features.
                    let mut next = d;
                    next.def.features.clear();
                    next.def.solids.clear();
                    next.input = PatternInput::Geometry;
                    self.pattern_draft = Some(next);
                } else {
                    self.pattern_draft = None;
                    self.selection = super::Selection::Body;
                }
            }
            Err(e) => {
                self.history.cancel();
                self.file_status = Some(Err(format!("{cmd}: {e}")));
            }
        }
    }
}

/// The ribbon icon of a pattern feature.
pub(super) fn icon_of(p: &PatternFeature) -> super::icons::Icon {
    use super::icons::Icon;
    match p.kind {
        PatternKind::Rectangular { .. } => Icon::PatternRect,
        PatternKind::Circular { .. } => Icon::PatternCirc,
        PatternKind::Mirror { .. } => Icon::Mirror,
        PatternKind::Sketch { .. } => Icon::SketchPattern,
    }
}

/// A one-line summary for the browser's hover text.
pub(super) fn summary(p: &PatternFeature) -> String {
    let what = if p.solid_mode {
        if p.solids.is_empty() {
            "all solids".to_owned()
        } else {
            format!("{} solid(s)", p.solids.len())
        }
    } else {
        format!("{} feature(s)", p.features.len())
    };
    match &p.kind {
        PatternKind::Sketch { sketch, .. } => format!(
            "{what} at the centre points of {}",
            sketch.as_deref().unwrap_or("no sketch")
        ),
        PatternKind::Rectangular { first, second } => {
            match second.as_ref().filter(|d| d.is_set()) {
                Some(s) => format!(
                    "{what}, {} x {:.2} mm by {} x {:.2} mm",
                    first.count, first.spacing, s.count, s.spacing
                ),
                None => format!("{what}, {} x {:.2} mm", first.count, first.spacing),
            }
        }
        PatternKind::Circular { count, angle, .. } => {
            format!("{what}, {count} over {:.2}°", angle.to_degrees())
        }
        PatternKind::Mirror { .. } => what,
    }
}

/// How a direction of a rectangular pattern is shown: its straight
/// direction or its path.
pub(super) fn describe_direction(d: &PatternDirection) -> Option<String> {
    use crate::part::PathRef;
    match (&d.dir, &d.path) {
        (_, Some(PathRef::Sketch { sketch, curves })) => {
            Some(format!("path: {} curve(s) of {sketch}", curves.len()))
        }
        (_, Some(PathRef::Edges(e))) => Some(format!("path: {} edge(s)", e.len())),
        (_, Some(PathRef::Sketch3d { sketch })) => Some(format!("path: {sketch}")),
        (Some(p), None) => Some(describe_pull(p)),
        (None, None) => None,
    }
}

/// How a point (Sketch Driven's Base Point) is shown in the panel.
pub(super) fn describe_point(p: &crate::part::PointRef) -> String {
    use crate::part::PointRef;
    match p {
        PointRef::Vertex(n) => format!("Vertex {n}"),
        PointRef::Work(id) => id.clone(),
        other => format!("{other:?}"),
    }
}

/// How a direction / axis / plane is shown in the panel.
pub(super) fn describe_pull(p: &PullRef) -> String {
    match p {
        PullRef::Plane(pl) => describe_plane(pl),
        PullRef::Line(l) => describe_line(l),
    }
}

pub(super) fn describe_plane(p: &PlaneRef) -> String {
    match p {
        PlaneRef::Origin(o) => format!("{o:?} Plane (normal)"),
        PlaneRef::Face(n) => n.clone(),
        PlaneRef::Work(id) | PlaneRef::Context(id) => id.clone(),
        PlaneRef::Fixed => "fixed".into(),
    }
}

pub(super) fn describe_line(l: &LineRef) -> String {
    match l {
        LineRef::Origin(k) => format!("{} Axis", ["X", "Y", "Z"][(*k).min(2)]),
        LineRef::Edge(n) => n.clone(),
        LineRef::Work(id) | LineRef::Context(id) => id.clone(),
    }
}
