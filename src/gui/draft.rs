//! The Face Draft command (`docs/spec/2.8-draft.md`): the selectors of the
//! type (Pull Direction, Fixed Plane or Parting Tool, Faces, Static Edges)
//! follow the shared flow (`command::flow`, `gui/command_ui.rs`): the
//! active one takes the clicks, a single pick moves on, Continue / Back /
//! Enter in the right-click menu and keys. Preview, OK / Apply.

use eframe::egui::{self, Color32, Rect, Stroke};

use super::CadCraftApp;
use super::command_ui::MenuOption;
use crate::command::flow::{Flow, PickKind, colors, draft_selectors};
use crate::kernel::{EdgeId, FaceId, Point3, TessellationOptions};
use crate::part::{
    DraftExtent, DraftFeature, DraftType, Feature, LineRef, PartingTool, PlaneRef, PullRef,
};

/// The selector that takes the next click.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DraftInput {
    /// Pull Direction (Fixed Edge, Parting Line).
    Pull,
    /// Fixed Plane.
    Plane,
    /// Parting Tool (Parting Line).
    Parting,
    /// Faces.
    Faces,
    /// Static Edges (Move Parting Line).
    Static,
}

impl DraftInput {
    fn key(self) -> &'static str {
        match self {
            DraftInput::Pull => "pull",
            DraftInput::Plane => "plane",
            DraftInput::Parting => "parting",
            DraftInput::Faces => "faces",
            DraftInput::Static => "static",
        }
    }

    fn from_key(k: &str) -> Option<Self> {
        Some(match k {
            "pull" => DraftInput::Pull,
            "plane" => DraftInput::Plane,
            "parting" => DraftInput::Parting,
            "faces" => DraftInput::Faces,
            "static" => DraftInput::Static,
            _ => return None,
        })
    }
}

/// What the cursor is over, as the active selector takes it.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum DraftPick {
    /// A face and the model point (Faces).
    Face(FaceId, Point3),
    /// A planar face or a work / origin plane.
    Plane(PlaneRef),
    /// A model edge (straight for a pull direction).
    Edge(EdgeId),
    /// A work axis.
    Axis(LineRef),
}

/// The open Face Draft command.
#[derive(Clone, Debug)]
pub(super) struct DraftDraft {
    /// The feature being defined (id and name are set on OK).
    pub def: DraftFeature,
    /// Edit Feature of this feature index.
    pub edit: Option<usize>,
    /// The active selector.
    pub input: DraftInput,
}

impl DraftDraft {
    /// Inventor's defaults: Fixed Edge, 1°, Pull Direction first.
    pub(super) fn new() -> Self {
        Self {
            def: DraftFeature::new(1f64.to_radians()),
            edit: None,
            input: DraftInput::Pull,
        }
    }

    pub(super) fn editing(def: DraftFeature, k: usize) -> Self {
        Self {
            def,
            edit: Some(k),
            input: DraftInput::Faces,
        }
    }

    /// The selectors of the type with the active one.
    pub(super) fn flow(&self) -> Flow {
        Flow::new(draft_selectors(&self.def), Some(self.input.key()))
    }

    /// Makes the selector `key` active.
    pub(super) fn set_active(&mut self, key: &str) {
        if let Some(i) = DraftInput::from_key(key) {
            self.input = i;
        }
    }

    /// The first selector of the type that still needs input.
    pub(super) fn first_input(&self) -> DraftInput {
        let f = Flow::new(draft_selectors(&self.def), None);
        f.active_key()
            .and_then(DraftInput::from_key)
            .unwrap_or(DraftInput::Faces)
    }

    /// Changes the type (the active selector follows).
    pub(super) fn set_type(&mut self, t: DraftType) {
        if self.def.draft_type != t {
            self.def.set_type(t);
            self.input = self.first_input();
        }
    }

    /// Whether the active selector takes faces with their click point.
    pub(super) fn picks_faces(&self) -> bool {
        self.input == DraftInput::Faces
    }

    /// A valid pick went in: a single selector moves on.
    fn picked(&mut self) {
        let mut f = self.flow();
        if f.picked()
            && let Some(k) = f.active_key()
        {
            self.set_active(k);
        }
    }

    /// The command's options in the right-click menu.
    pub(super) fn menu_options(&self) -> Vec<MenuOption> {
        let mut v: Vec<MenuOption> = [
            DraftType::FixedEdge,
            DraftType::FixedPlane,
            DraftType::PartingLine,
        ]
        .iter()
        .map(|t| MenuOption {
            label: t.label().to_owned(),
            checked: self.def.draft_type == *t,
        })
        .collect();
        for e in self.def.draft_type.extents() {
            v.push(MenuOption {
                label: e.label().to_owned(),
                checked: self.def.effective_extent() == *e,
            });
        }
        if self.def.draft_type.has_pull() {
            v.push(MenuOption {
                label: "Flip pull direction".to_owned(),
                checked: self.def.flip,
            });
        }
        v
    }

    /// Option `k` of [`Self::menu_options`] was chosen.
    pub(super) fn menu_option(&mut self, k: usize) {
        let types = [
            DraftType::FixedEdge,
            DraftType::FixedPlane,
            DraftType::PartingLine,
        ];
        if let Some(t) = types.get(k) {
            self.set_type(*t);
            return;
        }
        let extents = self.def.draft_type.extents();
        if let Some(e) = extents.get(k - types.len()) {
            self.def.extent = *e;
            return;
        }
        if self.def.draft_type.has_pull() {
            self.def.flip = !self.def.flip;
        }
    }
}

impl CadCraftApp {
    pub(super) fn start_draft(&mut self) {
        self.finish_sketch();
        self.mode = super::Mode::Normal;
        self.cancel_extrude();
        self.work_draft = None;
        self.blend_draft = None;
        self.sel.clear();
        self.face_sel = None;
        self.edge_sel = None;
        let mut d = DraftDraft::new();
        self.preset_start("Face Draft", &mut d.def);
        d.def.set_type(d.def.draft_type);
        d.input = d.first_input();
        self.draft_draft = Some(d);
    }

    /// What the active selector would take under `p` (prehighlight shows
    /// only that); `face` is the model face under the cursor.
    pub(super) fn draft_hover(
        &self,
        p: egui::Pos2,
        rect: Rect,
        face: Option<(FaceId, Point3)>,
    ) -> Option<DraftPick> {
        let d = self.draft_draft.as_ref()?;
        let flow = d.flow();
        let part = self.part();
        if flow.accepts(PickKind::Face) {
            return face.map(|(f, at)| DraftPick::Face(f, at));
        }
        if flow.accepts(PickKind::Edge) {
            return self.edge_under(p, rect).map(DraftPick::Edge);
        }
        if flow.accepts(PickKind::Axis)
            && let Some(super::work::WpInput::Line(l)) = self.work_axis_or_point_under(p, rect)
        {
            return Some(DraftPick::Axis(l));
        }
        if flow.accepts(PickKind::StraightEdge)
            && let Some(e) = self
                .edge_under(p, rect)
                .filter(|&e| part.body.edge_direction(e).is_some())
        {
            return Some(DraftPick::Edge(e));
        }
        if flow.accepts(PickKind::PlanarFace) || flow.accepts(PickKind::Plane) {
            // Only planes: a curved face has no normal to take.
            return self
                .plane_under(p, rect, flow.accepts(PickKind::Plane))
                .filter(|pl| crate::part::placement::plane_of(pl, &part.body, &part.work).is_ok())
                .map(DraftPick::Plane);
        }
        None
    }

    /// A click while the command is open, on what [`Self::draft_hover`]
    /// found.
    pub(super) fn draft_pick(&mut self, pick: DraftPick) {
        let body = self.part().body.clone();
        let Some(d) = self.draft_draft.as_mut() else {
            return;
        };
        match (d.input, pick) {
            (DraftInput::Faces, DraftPick::Face(f, at)) => d.def.toggle(&body, f, Some(at)),
            (DraftInput::Static, DraftPick::Edge(e)) => d.def.toggle_static(&body, e),
            (DraftInput::Pull, DraftPick::Edge(e)) => {
                if let Some(name) = body.edge(e).and_then(|x| x.name.clone()) {
                    d.def.pull = Some(PullRef::Line(LineRef::Edge(name)));
                    d.picked();
                }
            }
            (DraftInput::Pull, DraftPick::Axis(l)) => {
                d.def.pull = Some(PullRef::Line(l));
                d.picked();
            }
            (DraftInput::Pull, DraftPick::Plane(p)) => {
                d.def.pull = Some(PullRef::Plane(p));
                d.picked();
            }
            (DraftInput::Plane, DraftPick::Plane(p)) => {
                d.def.plane = Some(p);
                d.picked();
            }
            (DraftInput::Parting, DraftPick::Plane(p)) => {
                d.def.parting = Some(PartingTool::Plane(p));
                d.picked();
            }
            _ => {}
        }
    }

    /// A sketch chosen as the parting tool (browser / dialog list).
    pub(super) fn draft_parting_sketch(&mut self, id: String) {
        if let Some(d) = self.draft_draft.as_mut() {
            d.def.parting = Some(PartingTool::Sketch(id));
            d.input = DraftInput::Parting;
            d.picked();
        }
    }

    /// The lines of the parting sketch (Select Boundary).
    pub(super) fn draft_sketch_lines(
        &self,
    ) -> Option<Vec<(Point3, Point3, crate::kernel::Vector3)>> {
        let id = self.draft_draft.as_ref()?.def.parting_sketch()?.to_owned();
        let ps = self.part().sketches.iter().find(|s| s.id == id)?;
        Some(crate::part::draft::sketch_lines(&ps.sketch))
    }

    /// The faces to draft as they are on the current body.
    pub(super) fn draft_faces(&self) -> Vec<FaceId> {
        let Some(d) = &self.draft_draft else {
            return Vec::new();
        };
        let body = &self.part().body;
        d.def
            .faces
            .iter()
            .filter_map(|r| crate::part::shell::face_near(body, &r.face, Point3::from(r.hint)))
            .collect()
    }

    pub(super) fn draft_preview(&self) -> Result<Vec<Vec<Point3>>, String> {
        let Some(d) = &self.draft_draft else {
            return Err(String::new());
        };
        let key = format!("draft|{:?}|{:?}", d.edit, d.def);
        // Cached or still computing: no copy of the part this frame.
        if let Some(r) = self.preview_ready(&key) {
            return r;
        }
        let mut trial = self.part().clone();
        let (def, edit) = (d.def.clone(), d.edit);
        self.preview_async(key, move || {
            let built = match edit {
                Some(k) => trial.replace_feature(k, Feature::Draft(def)),
                None => trial.add_draft(def),
            };
            built
                .map(|id| Self::feature_polylines(&trial, &id))
                .map_err(|e| e.to_string())
        })
    }

    fn draw_edge(&self, painter: &egui::Painter, rect: Rect, e: EdgeId, c: [u8; 3], w: f32) {
        if let Ok(pts) = self
            .part()
            .body
            .sample_edge(e, &TessellationOptions::default())
        {
            self.draw_visible_polyline(
                painter,
                rect,
                &pts,
                Stroke::new(w, Color32::from_rgb(c[0], c[1], c[2])),
            );
        }
    }

    /// The command's glyphs in the view: the pull arrow (and the arrow of
    /// the pull candidate under the cursor), the fixed or parting plane's
    /// face outline, the fixed edge of each face (Fixed Edge; of the face
    /// under the cursor too) and the static edges.
    pub(super) fn draw_draft_overlay(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        hover: Option<&DraftPick>,
    ) {
        let Some(d) = &self.draft_draft else { return };
        let part = self.part();
        let body = &part.body;
        let pts: Vec<Point3> = body.vertices().map(|(_, v)| v.point).collect();
        if pts.is_empty() {
            return;
        }
        let c = pts
            .iter()
            .fold(Point3::origin(), |a, p| a + p.coords / pts.len() as f64);
        let size = pts.iter().map(|p| (p - c).norm()).fold(1.0, f64::max);
        let arrow = |dir: crate::kernel::Vector3, width: f32, col: Color32| {
            if let (Some(a), Some(b)) = (
                self.camera.project(&c, rect),
                self.camera.project(&(c + dir * size * 0.6), rect),
            ) {
                painter.arrow(a, b - a, Stroke::new(width, col));
            }
        };
        let [r, g, b] = colors::DIRECTION;
        let pull = d.def.pull_direction(body, &part.work);
        if let Ok(pull) = pull {
            arrow(pull, 2.5, Color32::from_rgb(r, g, b));
        }
        // The candidate's arrow, before the click.
        if d.input == DraftInput::Pull
            && let Some(h) = hover
        {
            let mut trial = d.def.clone();
            trial.pull = match h {
                DraftPick::Plane(p) => Some(PullRef::Plane(p.clone())),
                DraftPick::Axis(l) => Some(PullRef::Line(l.clone())),
                DraftPick::Edge(e) => body
                    .edge(*e)
                    .and_then(|x| x.name.clone())
                    .map(|n| PullRef::Line(LineRef::Edge(n))),
                DraftPick::Face(..) => None,
            };
            if let Ok(dir) = trial.pull_direction(body, &part.work) {
                arrow(dir, 1.5, Color32::from_rgba_unmultiplied(r, g, b, 150));
            }
        }
        // The fixed plane / parting tool on a face: its outline.
        let plane_face = match (&d.def.draft_type, &d.def.plane, &d.def.parting) {
            (DraftType::FixedPlane, Some(PlaneRef::Face(n)), _)
            | (DraftType::PartingLine, _, Some(PartingTool::Plane(PlaneRef::Face(n)))) => {
                body.find_face(n)
            }
            _ => None,
        };
        if let Some(f) = plane_face {
            for e in crate::part::select::edges_of_face(body, f) {
                self.draw_edge(painter, rect, e, colors::PLANE, 3.0);
            }
        }
        // Fixed Edge: the edge that stays, per face and under the cursor.
        if d.def.draft_type == DraftType::FixedEdge
            && let Ok(pull) = pull
        {
            let mut faces: Vec<(FaceId, Point3)> = d
                .def
                .faces
                .iter()
                .filter_map(|r| {
                    let at = Point3::from(r.hint);
                    crate::part::shell::face_near(body, &r.face, at).map(|f| (f, at))
                })
                .collect();
            if let Some(DraftPick::Face(f, at)) = hover {
                faces.push((*f, *at));
            }
            for (f, at) in faces {
                if let Some((e, _)) = crate::part::draft::fixed_edge(body, f, at, pull) {
                    self.draw_edge(painter, rect, e, colors::EDGES, 4.0);
                }
            }
        }
        for r in &d.def.static_edges {
            if let Some(e) = crate::part::draft::edge_of(body, r) {
                self.draw_edge(painter, rect, e, colors::EDGES, 4.0);
            }
        }
        if let Some(DraftPick::Edge(e)) = hover {
            self.draw_edge(painter, rect, *e, [60, 190, 90], 4.0);
        }
    }

    /// OK (`keep_open` false) or Apply.
    pub(super) fn commit_draft(&mut self, keep_open: bool) {
        let Some(mut d) = self.draft_draft.clone() else {
            return;
        };
        let cmd = if d.edit.is_some() {
            "Edit Feature"
        } else {
            "Face Draft"
        };
        self.preset_stamp("Face Draft", &mut d.def);
        self.checkpoint(cmd);
        let built = match d.edit {
            Some(k) => self
                .part_mut()
                .replace_feature(k, Feature::Draft(d.def.clone())),
            None => self.part_mut().add_draft(d.def.clone()),
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
                self.preset_used("Face Draft", &d.def, d.edit.is_none());
                if keep_open && d.edit.is_none() {
                    // Apply: the same pull / plane and angle, new faces.
                    let mut next = d;
                    next.def.faces.clear();
                    next.def.static_edges.clear();
                    next.input = DraftInput::Faces;
                    self.draft_draft = Some(next);
                } else {
                    self.draft_draft = None;
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

/// Whether an extent shows Draft Angle 2.
pub(super) fn shows_angle2(e: DraftExtent) -> bool {
    e == DraftExtent::Asymmetric
}
