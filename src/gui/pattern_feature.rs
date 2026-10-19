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
        }
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
                true
            }
            (PatternKind::Rectangular { second, .. }, PatternInput::Dir2) => {
                let Some(p) = pull else { return false };
                second.get_or_insert_with(PatternDirection::new).dir = Some(p);
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
        let line = edge
            .filter(|&e| body.edge_direction(e).is_some())
            .and_then(|e| body.edge(e).and_then(|x| x.name.clone()))
            .map(LineRef::Edge);
        if let Some(d) = self.pattern_draft.as_mut() {
            d.set_ref(line, plane);
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
        match &d.def.kind {
            PatternKind::Rectangular { first, second } => {
                if let Some(v) = first.dir.as_ref().and_then(|p| resolve(p, first.flip)) {
                    arrow(c, v, orange);
                }
                if let Some(s) = second
                    && let Some(v) = s.dir.as_ref().and_then(|p| resolve(p, s.flip))
                {
                    arrow(c, v, blue);
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
                    let k = d.edit.unwrap_or(self.part().last_inserted());
                    self.selection = super::Selection::Feature(k);
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
        PatternKind::Rectangular { first, second } => {
            match second.as_ref().filter(|d| d.dir.is_some()) {
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
        PlaneRef::Work(id) => id.clone(),
        PlaneRef::Fixed => "fixed".into(),
    }
}

pub(super) fn describe_line(l: &LineRef) -> String {
    match l {
        LineRef::Origin(k) => format!("{} Axis", ["X", "Y", "Z"][(*k).min(2)]),
        LineRef::Edge(n) => n.clone(),
        LineRef::Work(id) => id.clone(),
    }
}
