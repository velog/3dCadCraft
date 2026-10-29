//! The Sweep and Loft commands (`docs/spec/5.1-sweep.md`,
//! `docs/spec/5.2-loft.md`): selectors in the panel (Profiles → Path →
//! Guide Rail; Sections), picks in the graphics window, a preview computed
//! off the frame, OK / Apply / Cancel and Edit Feature.
//!
//! The selector logic is self-contained here so it can move to a shared
//! command-guidance module later.

use eframe::egui::{self, Pos2, Rect, RichText};

use super::CadCraftApp;
use super::icons::Icon;
use super::panels::{Fields, field_row, plabel, psection, warning_box};
use crate::kernel::Point3;
use crate::part::{
    Feature, LoftFeature, LoftSection, Operation, Output, PathRef, ProfileRef, ProfileScaling,
    SweepFeature, SweepOrientation, SweepType,
};

/// The active selector of the Sweep panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SweepInput {
    /// Profiles (regions of a sketch).
    Profiles,
    /// The path (sketch curves or edges).
    Path,
    /// The guide rail.
    Rail,
}

/// The open Sweep command.
#[derive(Clone, Debug)]
pub(super) struct SweepDraft {
    /// The feature being defined.
    pub def: SweepFeature,
    /// Index of the profile sketch, once a profile is picked.
    pub sketch: Option<usize>,
    /// The active selector.
    pub input: SweepInput,
    /// Edit Feature of this feature index.
    pub edit: Option<usize>,
    /// Optimize for Single Selection: one pick moves to the next selector.
    pub single: bool,
    /// Keep the path sketch visible after Apply (+).
    pub keep_sketch_visible: bool,
}

/// The open Loft command (its panel and selector flow: `gui/loft.rs`).
#[derive(Clone, Debug)]
pub(super) struct LoftDraft {
    /// The feature being defined.
    pub def: LoftFeature,
    /// Edit Feature of this feature index.
    pub edit: Option<usize>,
    /// The active selector's key (`command::flow::loft_selectors`).
    pub input: &'static str,
    /// The open tab.
    pub tab: super::loft::LoftTab,
    /// The point set being picked (Transition tab).
    pub map_set: usize,
}

const PICK: egui::Color32 = egui::Color32::from_rgb(60, 190, 90);
const CHOSEN: egui::Color32 = egui::Color32::from_rgb(40, 120, 230);

impl CadCraftApp {
    /// Starts Sweep: the only profile of the only visible sketch is taken
    /// (Inventor), then the Path selector is active.
    pub(super) fn start_sweep(&mut self) {
        self.finish_sketch();
        self.mode = super::Mode::Normal;
        self.cancel_extrude();
        self.work_draft = None;
        self.blend_draft = None;
        self.face_sel = None;
        self.edge_sel = None;
        let mut d = SweepDraft {
            def: SweepFeature::new(Vec::new(), PathRef::Edges(Vec::new())),
            sketch: None,
            input: SweepInput::Profiles,
            edit: None,
            single: true,
            keep_sketch_visible: false,
        };
        let candidates: Vec<usize> = (0..self.part().sketches.len())
            .filter(|&i| {
                let ps = &self.part().sketches[i];
                ps.visible && self.sketch_regions(&ps.sketch).len() == 1
            })
            .collect();
        if let [i] = candidates[..] {
            let r = self.sketch_regions(&self.part().sketches[i].sketch);
            d.def.profiles = vec![ProfileRef::of(&r[0], r[0].interior_point())];
            d.sketch = Some(i);
            d.input = SweepInput::Path;
        }
        self.sweep_draft = Some(d);
    }

    /// Starts Loft (sections are clicked in order).
    pub(super) fn start_loft(&mut self) {
        self.finish_sketch();
        self.mode = super::Mode::Normal;
        self.cancel_extrude();
        self.work_draft = None;
        self.blend_draft = None;
        self.face_sel = None;
        self.edge_sel = None;
        self.loft_draft = Some(LoftDraft::new());
    }

    /// Edit Feature of a sweep: the panel with its inputs, rolled back.
    pub(super) fn edit_sweep(&mut self, def: SweepFeature, k: usize) {
        let sketch = self.part().sketches.iter().position(|s| s.id == def.sketch);
        self.roll_back_to(k);
        self.sweep_draft = Some(SweepDraft {
            def,
            sketch,
            input: SweepInput::Path,
            edit: Some(k),
            single: true,
            keep_sketch_visible: false,
        });
    }

    /// Edit Feature of a loft.
    pub(super) fn edit_loft(&mut self, def: LoftFeature, k: usize) {
        self.roll_back_to(k);
        self.loft_draft = Some(LoftDraft::editing(def, k));
    }

    /// Whether Sweep or Loft is open (the view is not idle).
    pub(super) fn sweep_loft_open(&self) -> bool {
        self.sweep_draft.is_some() || self.loft_draft.is_some()
    }

    /// The status-bar prompt of the open Sweep / Loft.
    pub(super) fn sweep_loft_prompt(&self) -> Option<String> {
        if let Some(d) = &self.sweep_draft {
            let text = match d.input {
                SweepInput::Profiles if d.def.profiles.is_empty() => {
                    "Sweep: click the profile (a closed region of a sketch)"
                }
                SweepInput::Profiles => {
                    "Sweep: click more profiles (a second click removes one), then pick the Path"
                }
                SweepInput::Path => {
                    "Sweep: click the path (a sketch curve on another plane, or model edges)"
                }
                SweepInput::Rail => "Sweep: click the guide rail curve, then OK",
            };
            return Some(super::i18n::tr(text).into_owned());
        }
        self.loft_draft.as_ref().and_then(|_| self.command_prompt())
    }

    /// Picks and highlights in the graphics window while Sweep or Loft is
    /// open: the hovered region / curve / edge in green, the chosen inputs
    /// in blue, the preview's edges in orange.
    pub(super) fn sweep_loft_view(
        &mut self,
        painter: &egui::Painter,
        rect: Rect,
        pointer: Option<Pos2>,
        clicked: bool,
    ) {
        if !self.sweep_loft_open() {
            return;
        }
        // Chosen paths and sections.
        for pts in self.chosen_polylines() {
            self.draw_visible_polyline(painter, rect, &pts, egui::Stroke::new(3.0, CHOSEN));
        }
        if let Some(p) = pointer {
            let hover = self.sweep_loft_hover(p, rect);
            if let Some((_, pts)) = &hover {
                for line in pts {
                    self.draw_visible_polyline(painter, rect, line, egui::Stroke::new(3.0, PICK));
                }
            }
            if clicked && let Some((pick, _)) = hover {
                self.sweep_loft_pick(pick);
            }
        }
        let preview = if self.sweep_draft.is_some() {
            self.sweep_preview()
        } else {
            self.loft_preview()
        };
        if let Ok(edges) = preview {
            let stroke = egui::Stroke::new(2.0, egui::Color32::from_rgb(230, 120, 20));
            for e in edges {
                let pts: Vec<_> = e
                    .iter()
                    .filter_map(|p| self.camera.project(p, rect))
                    .collect();
                painter.add(egui::Shape::line(pts, stroke));
            }
        }
    }

    /// The polylines of the chosen path, rail and sections.
    fn chosen_polylines(&self) -> Vec<Vec<Point3>> {
        let part = self.part();
        let mut out = Vec::new();
        let mut path = |r: &PathRef| {
            if let Ok(g) = r.resolve(&part.sketches, &part.body, Point3::origin()) {
                out.push(g.polyline());
            }
        };
        if let Some(d) = &self.sweep_draft {
            path(&d.def.path);
            if let Some(r) = &d.def.rail {
                path(r);
            }
        }
        if let Some(d) = &self.loft_draft {
            for r in d.def.rails.iter().chain(d.def.center_line.iter()) {
                path(r);
            }
            for m in &d.def.map_points {
                for q in &m.points {
                    let c = Point3::from(*q);
                    let e = 0.6;
                    out.push(vec![
                        c - crate::kernel::Vector3::x() * e,
                        c + crate::kernel::Vector3::x() * e,
                    ]);
                    out.push(vec![
                        c - crate::kernel::Vector3::y() * e,
                        c + crate::kernel::Vector3::y() * e,
                    ]);
                }
            }
            for s in &d.def.sections {
                if let LoftSection::Profile { sketch, profile } = s
                    && let Some(ps) = part.sketches.iter().find(|p| &p.id == sketch)
                    && let Some(r) = profile.resolve(&self.sketch_regions(&ps.sketch))
                {
                    let pl = &ps.sketch.plane;
                    let mut pts: Vec<Point3> = r
                        .outer
                        .polygon(1e-2)
                        .iter()
                        .map(|q| pl.point_at(q.x, q.y))
                        .collect();
                    if let Some(f) = pts.first().copied() {
                        pts.push(f);
                    }
                    out.push(pts);
                }
            }
        }
        out
    }

    /// What a click at `p` would pick, with its outline.
    fn sweep_loft_hover(&self, p: Pos2, rect: Rect) -> Option<(Pick, Vec<Vec<Point3>>)> {
        use super::overlay;
        use crate::command::flow::PickKind;
        let part = self.part();
        let loft_flow = self.loft_draft.as_ref().map(|d| d.flow());
        let loft_takes = |k: PickKind| loft_flow.as_ref().is_some_and(|f| f.accepts(k));
        // Loft, Map Point: the nearest point of the next section's outline.
        if loft_takes(PickKind::SectionPoint)
            && let Some(d) = &self.loft_draft
        {
            let k = d
                .def
                .map_points
                .get(d.map_set)
                .map_or(0, |m| m.points.len());
            let (plane, pts) = d.def.section_outline(&part.sketches, &part.body, k)?;
            let world = self.camera.ray_plane(p, rect, &plane)?;
            let q = nearest_on_polyline(&pts, &world)?;
            let e = 0.8;
            let x = plane.x_axis.into_inner() * e;
            let y = plane.y_axis().into_inner() * e;
            return Some((
                Pick::MapPoint([q.x, q.y, q.z]),
                vec![vec![q - x, q + x], vec![q - y, q + y], pts],
            ));
        }
        let want_region = match &self.sweep_draft {
            Some(d) => d.input == SweepInput::Profiles,
            None => loft_takes(PickKind::Region),
        };
        let sketch_on = self.cat_on(crate::command::selection::Category::SketchEntity);
        // Loft: a sketch point is a point section.
        if loft_takes(PickKind::SketchPoint) && sketch_on {
            for ps in part.sketches.iter().filter(|s| s.visible) {
                if let Some(pt) = overlay::pick_any_point(
                    &self.camera,
                    rect,
                    &ps.sketch,
                    p,
                    overlay::POINT_PICK_PX,
                ) && let Some(q) = ps.sketch.point(pt)
                {
                    let w = ps.sketch.plane.point_at(q.position.x, q.position.y);
                    return Some((Pick::Point(ps.id.clone(), pt), vec![vec![w, w]]));
                }
            }
        }
        if want_region {
            for (i, ps) in part
                .sketches
                .iter()
                .enumerate()
                .filter(|(_, s)| s.visible && sketch_on)
            {
                let sk = &ps.sketch;
                let Some(world) = self.camera.ray_plane(p, rect, &sk.plane) else {
                    continue;
                };
                let uv = sk.plane.project(&world);
                let regions = self.sketch_regions(sk);
                if let Some(r) = regions
                    .iter()
                    .filter(|r| r.contains(uv))
                    .min_by(|a, b| a.area().total_cmp(&b.area()))
                {
                    let mut pts: Vec<Point3> = r
                        .outer
                        .polygon(1e-2)
                        .iter()
                        .map(|q| sk.plane.point_at(q.x, q.y))
                        .collect();
                    if let Some(f) = pts.first().copied() {
                        pts.push(f);
                    }
                    return Some((
                        Pick::Region(i, ProfileRef::of(r, r.interior_point())),
                        vec![pts],
                    ));
                }
            }
            // Loft: an edge (of a surface) is part of an edge section; edges come before faces.
            if loft_takes(PickKind::Edge)
                && let Some(e) = self.edge_under(p, rect)
                && let Some(face) = self
                    .picker()
                    .faces_of(e)
                    .into_iter()
                    .find_map(|f| part.body.face(f).and_then(|x| x.name.clone()))
                && let Some(at) = self.click_point(&super::select::SelItem::Edge(e), p, rect)
            {
                let pts = part
                    .body
                    .sample_edge(e, &crate::kernel::TessellationOptions::default())
                    .unwrap_or_default();
                return Some((
                    Pick::SurfEdge(crate::part::surface_op::SurfEdge {
                        face,
                        hint: [at.x, at.y, at.z],
                    }),
                    vec![pts],
                ));
            }
            // Loft: a planar face is a face-loop section.
            if loft_takes(PickKind::PlanarFace)
                && let Some((o, dir)) = self.camera.ray(p, rect)
                && let Some(dir) = dir.try_normalize(1e-12)
                && let Some((f, t)) = self.face_hit(&o, &dir)
                && part.body.face_plane(f).is_some()
                && let Some(name) = part.body.face(f).and_then(|x| x.name.clone())
            {
                let at = o + dir * t;
                return Some((
                    Pick::Face(crate::part::FaceRef {
                        face: name,
                        hint: [at.x, at.y, at.z],
                    }),
                    Vec::new(),
                ));
            }
            return None;
        }
        if self.loft_draft.is_some() && !loft_takes(PickKind::SketchCurve) {
            return None;
        }
        // Path / rail: a sketch curve (its connected chain) or an edge.
        let profile_sketch = self.sweep_draft.as_ref().and_then(|d| d.sketch);
        for (i, ps) in part.sketches.iter().enumerate() {
            if !ps.visible || Some(i) == profile_sketch || !sketch_on {
                continue;
            }
            if let Some((e, _)) =
                overlay::pick_entity(&self.camera, rect, &ps.sketch, p, overlay::CURVE_SNAP_PX)
            {
                let (chain, _) = ps.sketch.chain(e);
                let curves: Vec<_> = chain.into_iter().map(|(id, _)| id).collect();
                let r = PathRef::Sketch {
                    sketch: ps.id.clone(),
                    curves,
                };
                let pts = r
                    .resolve(&part.sketches, &part.body, Point3::origin())
                    .map(|g| vec![g.polyline()])
                    .unwrap_or_default();
                return Some((Pick::Path(r), pts));
            }
        }
        let e = self.edge_under(p, rect)?;
        let er = crate::part::blend::edge_ref(&part.body, e)?;
        let pts = part
            .body
            .sample_edge(e, &crate::kernel::TessellationOptions::default())
            .ok()?;
        Some((Pick::Edge(er), vec![pts]))
    }

    /// Applies a pick to the active selector (and moves on with Optimize
    /// for Single Selection).
    fn sweep_loft_pick(&mut self, pick: Pick) {
        if let Some(d) = self.loft_draft.as_mut()
            && d.input != "sections"
        {
            match (d.input, pick) {
                ("rails", Pick::Path(r)) => {
                    if let Some(k) = d.def.rails.iter().position(|x| x == &r) {
                        d.def.rails.remove(k);
                    } else {
                        d.def.rails.push(r);
                    }
                }
                ("rails", Pick::Edge(e)) => {
                    let r = PathRef::Edges(vec![e]);
                    if let Some(k) = d.def.rails.iter().position(|x| x == &r) {
                        d.def.rails.remove(k);
                    } else {
                        d.def.rails.push(r);
                    }
                }
                ("center", Pick::Path(r)) => {
                    d.def.center_line = Some(r);
                    d.picked();
                }
                ("center", Pick::Edge(e)) => {
                    let mut list = match &d.def.center_line {
                        Some(PathRef::Edges(v)) => v.clone(),
                        _ => Vec::new(),
                    };
                    if let Some(k) = list.iter().position(|x| x == &e) {
                        list.remove(k);
                    } else {
                        list.push(e);
                    }
                    d.def.center_line = Some(PathRef::Edges(list));
                }
                ("map", Pick::MapPoint(q)) => {
                    let n = d.def.sections.len();
                    if let Some(m) = d.def.map_points.get_mut(d.map_set)
                        && m.points.len() < n
                    {
                        m.points.push(q);
                    }
                }
                _ => {}
            }
            return;
        }
        if self.loft_draft.is_some() {
            let sec = match pick {
                Pick::Region(i, pr) => match self.part().sketches.get(i) {
                    Some(ps) => LoftSection::Profile {
                        sketch: ps.id.clone(),
                        profile: pr,
                    },
                    None => return,
                },
                Pick::Point(sketch, point) => LoftSection::Point { sketch, point },
                Pick::Face(f) => LoftSection::FaceLoop(f),
                Pick::SurfEdge(e) => {
                    self.loft_pick_edge(e);
                    return;
                }
                Pick::Path(_) | Pick::Edge(_) | Pick::MapPoint(_) => return,
            };
            let Some(d) = self.loft_draft.as_mut() else {
                return;
            };
            let same = |a: &LoftSection, b: &LoftSection| match (a, b) {
                (
                    LoftSection::Profile {
                        sketch: s1,
                        profile: p1,
                    },
                    LoftSection::Profile {
                        sketch: s2,
                        profile: p2,
                    },
                ) => s1 == s2 && p1.sources == p2.sources,
                (x, y) => x == y,
            };
            if let Some(k) = d.def.sections.iter().position(|s| same(s, &sec)) {
                d.def.sections.remove(k);
            } else {
                d.def.sections.push(sec);
            }
            return;
        }
        let Some(d) = self.sweep_draft.as_mut() else {
            return;
        };
        match (d.input, pick) {
            (SweepInput::Profiles, Pick::Region(i, pr)) => {
                if d.sketch != Some(i) {
                    d.sketch = Some(i);
                    d.def.profiles = vec![pr];
                } else if let Some(k) = d.def.profiles.iter().position(|p| p.sources == pr.sources)
                {
                    d.def.profiles.remove(k);
                } else {
                    d.def.profiles.push(pr);
                }
                if d.single && !d.def.profiles.is_empty() {
                    d.input = SweepInput::Path;
                }
            }
            (SweepInput::Path, Pick::Path(r)) => {
                d.def.path = r;
                if d.def.sweep_type == SweepType::PathAndGuideRail && d.single {
                    d.input = SweepInput::Rail;
                }
            }
            (SweepInput::Path, Pick::Edge(e)) => {
                let mut list = match &d.def.path {
                    PathRef::Edges(v) => v.clone(),
                    PathRef::Sketch { .. } => Vec::new(),
                };
                if let Some(k) = list.iter().position(|x| x == &e) {
                    list.remove(k);
                } else {
                    list.push(e);
                }
                d.def.path = PathRef::Edges(list);
            }
            (SweepInput::Rail, Pick::Path(r)) => d.def.rail = Some(r),
            (SweepInput::Rail, Pick::Edge(e)) => {
                let mut list = match &d.def.rail {
                    Some(PathRef::Edges(v)) => v.clone(),
                    _ => Vec::new(),
                };
                if let Some(k) = list.iter().position(|x| x == &e) {
                    list.remove(k);
                } else {
                    list.push(e);
                }
                d.def.rail = Some(PathRef::Edges(list));
            }
            _ => {}
        }
    }

    /// The sweep's result as edge polylines (off the frame), or why not.
    pub(super) fn sweep_preview(&self) -> Result<Vec<Vec<Point3>>, String> {
        let Some(d) = &self.sweep_draft else {
            return Err(String::new());
        };
        let Some(sketch) = d.sketch else {
            return Err("Select a profile".into());
        };
        if d.def.profiles.is_empty() {
            return Err("Select a profile".into());
        }
        if d.def.path.is_empty() {
            return Err("Select the path".into());
        }
        let key = format!(
            "sweep|{:?}|{sketch}|{}",
            d.edit,
            serde_json::to_string(&d.def).unwrap_or_default()
        );
        if let Some(r) = self.preview_ready(&key) {
            return r;
        }
        let mut trial = self.part().clone();
        let (def, edit) = (d.def.clone(), d.edit);
        self.preview_async(key, move || {
            let built = match edit {
                Some(k) => trial.replace_feature(k, Feature::Sweep(def)),
                None => trial.add_sweep(sketch, def),
            };
            built
                .map(|id| Self::feature_polylines(&trial, &id))
                .map_err(|e| e.to_string())
        })
    }

    /// The loft's result as edge polylines (off the frame), or why not.
    pub(super) fn loft_preview(&self) -> Result<Vec<Vec<Point3>>, String> {
        let Some(d) = &self.loft_draft else {
            return Err(String::new());
        };
        if d.def.sections.len() < 2 {
            return Err("Select at least two sections".into());
        }
        let key = format!(
            "loft|{:?}|{}",
            d.edit,
            serde_json::to_string(&d.def).unwrap_or_default()
        );
        if let Some(r) = self.preview_ready(&key) {
            return r;
        }
        let mut trial = self.part().clone();
        let (def, edit) = (d.def.clone(), d.edit);
        self.preview_async(key, move || {
            let built = match edit {
                Some(k) => trial.replace_feature(k, Feature::Loft(def)),
                None => trial.add_loft(def),
            };
            built
                .map(|id| Self::feature_polylines(&trial, &id))
                .map_err(|e| e.to_string())
        })
    }

    /// OK (`keep_open` false) or Apply (+) of the Sweep.
    pub(super) fn commit_sweep(&mut self, keep_open: bool) {
        let Some(d) = self.sweep_draft.clone() else {
            return;
        };
        let Some(sketch) = d.sketch else {
            self.file_status = Some(Err("Sweep: select a profile".into()));
            return;
        };
        let cmd = if d.edit.is_some() {
            "Edit Feature"
        } else {
            "Sweep"
        };
        self.checkpoint(cmd);
        let built = match d.edit {
            Some(k) => self
                .part_mut()
                .replace_feature(k, Feature::Sweep(d.def.clone())),
            None => self.part_mut().add_sweep(sketch, d.def.clone()),
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
                    // Apply (+): a new sweep with the same settings; the path
                    // sketch stays visible when asked.
                    if d.keep_sketch_visible
                        && let Some(id) = d.def.path.sketch_id().map(str::to_owned)
                        && let Some(ps) = self.part_mut().sketches.iter_mut().find(|s| s.id == id)
                    {
                        ps.visible = true;
                    }
                    let mut next = d;
                    next.def.profiles.clear();
                    next.sketch = None;
                    next.input = SweepInput::Profiles;
                    self.sweep_draft = Some(next);
                } else {
                    self.sweep_draft = None;
                    let k = d.edit.unwrap_or_else(|| self.part().last_inserted());
                    self.selection = super::Selection::Feature(k);
                }
            }
            Err(e) => {
                self.history.cancel();
                self.file_status = Some(Err(format!("{cmd}: {e}")));
            }
        }
    }

    /// OK (`keep_open` false) or Apply of the Loft.
    pub(super) fn commit_loft(&mut self, keep_open: bool) {
        let Some(d) = self.loft_draft.clone() else {
            return;
        };
        let cmd = if d.edit.is_some() {
            "Edit Feature"
        } else {
            "Loft"
        };
        self.checkpoint(cmd);
        let built = match d.edit {
            Some(k) => self
                .part_mut()
                .replace_feature(k, Feature::Loft(d.def.clone())),
            None => self.part_mut().add_loft(d.def.clone()),
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
                    let mut next = d;
                    next.def.sections.clear();
                    self.loft_draft = Some(next);
                } else {
                    self.loft_draft = None;
                    let k = d.edit.unwrap_or_else(|| self.part().last_inserted());
                    self.selection = super::Selection::Feature(k);
                }
            }
            Err(e) => {
                self.history.cancel();
                self.file_status = Some(Err(format!("{cmd}: {e}")));
            }
        }
    }

    /// The Sweep panel.
    pub(super) fn sweep_dialog(&mut self, ui: &mut egui::Ui) {
        let Some(mut d) = self.sweep_draft.clone() else {
            return;
        };
        self.panel_header(ui, Icon::Sweep, "Sweep");
        let part = self.part();
        let bodies = part.bodies_before(d.edit.unwrap_or(part.insert_index()));
        let path_text = describe_path(&d.def.path, part);
        let rail_text = d
            .def
            .rail
            .as_ref()
            .map_or_else(|| "none".to_owned(), |r| describe_path(r, part));
        psection(ui, "sweep_input", "Input Geometry", |ui| {
            selector_row(
                ui,
                "Profiles",
                &format!("{} selected", d.def.profiles.len()),
                &mut d.input,
                SweepInput::Profiles,
                "Click closed regions of a sketch (a second click removes one)",
            );
            selector_row(
                ui,
                "Path",
                &path_text,
                &mut d.input,
                SweepInput::Path,
                "Click a sketch curve on another plane (its connected chain is taken) or model edges",
            );
            if d.def.sweep_type == SweepType::PathAndGuideRail {
                selector_row(
                    ui,
                    "Guide Rail",
                    &rail_text,
                    &mut d.input,
                    SweepInput::Rail,
                    "A second curve that turns and scales the profile",
                );
            }
            if bodies.len() > 1 {
                solids_row(ui, &bodies, &mut d.def.solids);
            }
        });
        psection(ui, "sweep_type", "Type", |ui| {
            plabel(ui, "Type");
            ui.vertical(|ui| {
                ui.radio_value(&mut d.def.sweep_type, SweepType::Path, "Path");
                ui.radio_value(
                    &mut d.def.sweep_type,
                    SweepType::PathAndGuideRail,
                    "Path & Guide Rail",
                );
                ui.add_enabled(false, egui::RadioButton::new(false, "Path & Guide Surface"))
                    .on_disabled_hover_text("Not supported yet (docs/spec/5.1-sweep.md)");
            });
            ui.end_row();
            match d.def.sweep_type {
                SweepType::PathAndGuideRail => {
                    plabel(ui, "Scaling");
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut d.def.scaling, ProfileScaling::XY, "X & Y")
                            .on_hover_text("The profile grows with the distance to the rail");
                        ui.selectable_value(&mut d.def.scaling, ProfileScaling::X, "X")
                            .on_hover_text("Only toward the rail");
                        ui.selectable_value(&mut d.def.scaling, ProfileScaling::None, "None")
                            .on_hover_text("The rail only turns the profile");
                    });
                    ui.end_row();
                }
                _ => {
                    plabel(ui, "Orientation");
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut d.def.orientation, SweepOrientation::Path, "Path")
                            .on_hover_text("The profile keeps its angle to the path");
                        ui.selectable_value(
                            &mut d.def.orientation,
                            SweepOrientation::Parallel,
                            "Parallel",
                        )
                        .on_hover_text("The profile stays parallel to where it started");
                    });
                    ui.end_row();
                }
            }
        });
        let table = self.part().parameter_table();
        let mut fx = Fields {
            table: &table,
            key: "sweep_draft".into(),
            commits: Vec::new(),
            bodies: Vec::new(),
        };
        let snapshot = Feature::Sweep(d.def.clone());
        if d.def.tapers() {
            psection(ui, "sweep_taper", "Taper / Twist", |ui| {
                field_row(ui, &mut fx, "Taper", &snapshot, "taper");
                field_row(ui, &mut fx, "Twist", &snapshot, "twist");
            });
        }
        let commits = std::mem::take(&mut fx.commits);
        drop(fx);
        for (field, text) in commits {
            let mut f = Feature::Sweep(d.def.clone());
            match self.part().apply_draft_field(&mut f, field, &text) {
                Ok(()) => {
                    if let Feature::Sweep(s) = f {
                        d.def = s;
                    }
                    self.file_status = None;
                }
                Err(e) => self.file_status = Some(Err(format!("Sweep: {e}"))),
            }
        }
        psection(ui, "sweep_output", "Output", |ui| {
            output_rows(ui, &mut d.def.output, &mut d.def.operation);
            plabel(ui, "Solid Sweep");
            ui.add_enabled(
                false,
                egui::Checkbox::new(&mut false, "Toolbody along the path"),
            )
            .on_disabled_hover_text("Not supported yet (docs/spec/5.1-sweep.md)");
            ui.end_row();
        });
        psection(ui, "sweep_advanced", "Advanced", |ui| {
            plabel(ui, "Selection");
            ui.checkbox(&mut d.single, "Optimize for Single Selection")
                .on_hover_text("One pick moves on to the next selector");
            ui.end_row();
            plabel(ui, "Apply (+)");
            ui.checkbox(&mut d.keep_sketch_visible, "Keep sketch visible")
                .on_hover_text("After Apply the path sketch stays visible for the next sweep");
            ui.end_row();
        });
        self.sweep_draft = Some(d.clone());
        let preview = self.sweep_preview();
        let ok = buttons(ui, &preview, d.edit.is_none());
        match ok {
            Some(Button::Ok) => self.commit_sweep(false),
            Some(Button::Apply) => self.commit_sweep(true),
            Some(Button::Cancel) => {
                self.sweep_draft = None;
                self.end_feature_edit();
            }
            None => {}
        }
    }

    /// Properties of a Sweep in the browser selection.
    pub(super) fn sweep_properties(&mut self, ui: &mut egui::Ui, k: usize, s: SweepFeature) {
        self.panel_header(ui, Icon::Sweep, &s.name);
        if let Some(h) = self.part().feature_health.get(&s.id) {
            warning_box(ui, h);
        }
        let part = self.part();
        let sketch = part
            .sketches
            .iter()
            .find(|p| p.id == s.sketch)
            .map_or_else(|| format!("{} (missing)", s.sketch), |p| p.name.clone());
        let path = describe_path(&s.path, part);
        psection(ui, &format!("sweep_props_{k}"), "Input Geometry", |ui| {
            plabel(ui, "Sketch");
            ui.label(sketch);
            ui.end_row();
            plabel(ui, "Profiles");
            ui.label(format!("{}", s.profiles.len()));
            ui.end_row();
            plabel(ui, "Path");
            ui.label(path);
            ui.end_row();
            plabel(ui, "Type");
            ui.label(match s.sweep_type {
                SweepType::Path => "Path",
                SweepType::PathAndGuideRail => "Path & Guide Rail",
                SweepType::PathAndGuideSurface => "Path & Guide Surface",
            });
            ui.end_row();
            plabel(ui, "Operation");
            ui.label(s.operation.label());
            ui.end_row();
        });
        if ui.button("Edit Feature").clicked() {
            self.edit_feature_dialog(k);
        }
    }

    /// Properties of a Loft in the browser selection.
    pub(super) fn loft_properties(&mut self, ui: &mut egui::Ui, k: usize, l: LoftFeature) {
        self.panel_header(ui, Icon::Loft, &l.name);
        if let Some(h) = self.part().feature_health.get(&l.id) {
            warning_box(ui, h);
        }
        let part = self.part();
        let names: Vec<String> = l
            .sections
            .iter()
            .map(|s| describe_section(s, part))
            .collect();
        psection(ui, &format!("loft_props_{k}"), "Sections", |ui| {
            for (i, n) in names.iter().enumerate() {
                plabel(ui, &format!("{}", i + 1));
                ui.label(n);
                ui.end_row();
            }
            plabel(ui, "Guide");
            ui.label(match l.guide {
                crate::part::LoftGuide::Rails if l.rails.is_empty() => "none".to_owned(),
                crate::part::LoftGuide::Rails => format!("{} rail(s)", l.rails.len()),
                crate::part::LoftGuide::CenterLine => "Center Line".to_owned(),
                crate::part::LoftGuide::AreaLoft => {
                    format!("Area Loft, {} placed section(s)", l.placed.len())
                }
            });
            ui.end_row();
            plabel(ui, "Operation");
            ui.label(l.operation.label());
            ui.end_row();
        });
        if ui.button("Edit Feature").clicked() {
            self.edit_feature_dialog(k);
        }
    }
}

/// A pick in the graphics window.
#[derive(Clone, Debug)]
enum Pick {
    Region(usize, ProfileRef),
    Path(PathRef),
    Edge(crate::part::EdgeRef),
    Point(String, crate::sketch::PointId),
    Face(crate::part::FaceRef),
    /// An edge of the model for a loft's edge section (a face along it + the point picked).
    SurfEdge(crate::part::surface_op::SurfEdge),
    /// A point on a loft section's outline (model space).
    MapPoint([crate::kernel::Real; 3]),
}

impl CadCraftApp {
    /// An edge picked for a loft section: added to the last section when that is a chain of
    /// edges of the same body (the same edge again drops it), else a new edge section.
    fn loft_pick_edge(&mut self, e: crate::part::surface_op::SurfEdge) {
        let body_of = |app: &Self, x: &crate::part::surface_op::SurfEdge| {
            app.body_of_face(&x.face, Some(x.hint))
        };
        let mine = body_of(self, &e);
        let last_body = match self.loft_draft.as_ref().and_then(|d| d.def.sections.last()) {
            Some(LoftSection::Edges(list)) => list.first().and_then(|x| body_of(self, x)),
            _ => None,
        };
        let near = |a: &[crate::kernel::Real; 3], b: &[crate::kernel::Real; 3]| {
            (Point3::from(*a) - Point3::from(*b)).norm() < 1e-3
        };
        let Some(d) = self.loft_draft.as_mut() else {
            return;
        };
        // The same edge again (same face, a point on it near the first): dropped.
        for k in 0..d.def.sections.len() {
            if let LoftSection::Edges(list) = &mut d.def.sections[k]
                && let Some(i) = list
                    .iter()
                    .position(|x| x.face == e.face && near(&x.hint, &e.hint))
            {
                list.remove(i);
                if list.is_empty() {
                    d.def.sections.remove(k);
                }
                return;
            }
        }
        match d.def.sections.last_mut() {
            Some(LoftSection::Edges(list)) if mine.is_some() && mine == last_body => list.push(e),
            _ => d.def.sections.push(LoftSection::Edges(vec![e])),
        }
    }
}

/// The point of polyline `pts` nearest `q`.
fn nearest_on_polyline(pts: &[Point3], q: &Point3) -> Option<Point3> {
    pts.windows(2)
        .map(|w| {
            let d = w[1] - w[0];
            let l2 = d.norm_squared();
            let t = if l2 > 0.0 {
                ((q - w[0]).dot(&d) / l2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            w[0] + d * t
        })
        .min_by(|a, b| (a - q).norm().total_cmp(&(b - q).norm()))
}

#[derive(Clone, Copy)]
pub(super) enum Button {
    Ok,
    Apply,
    Cancel,
}

/// A selector row: a toggle that is lit while it is the active selector.
fn selector_row(
    ui: &mut egui::Ui,
    label: &str,
    text: &str,
    active: &mut SweepInput,
    me: SweepInput,
    tip: &str,
) {
    plabel(ui, label);
    let on = *active == me;
    let button = egui::Button::new(if on {
        RichText::new(text).strong()
    } else {
        RichText::new(text)
    })
    .selected(on);
    if ui.add(button).on_hover_text(tip).clicked() {
        *active = me;
    }
    ui.end_row();
}

pub(super) fn solids_row(ui: &mut egui::Ui, bodies: &[(String, String)], solids: &mut Vec<String>) {
    plabel(ui, "Solids");
    let current = solids.first().cloned();
    let shown = current
        .as_ref()
        .and_then(|c| bodies.iter().find(|(id, _)| id == c))
        .map_or("Default".to_owned(), |(_, n)| n.clone());
    egui::ComboBox::from_id_salt(("sweep_solids", bodies.len()))
        .selected_text(shown)
        .show_ui(ui, |ui| {
            if ui.selectable_label(current.is_none(), "Default").clicked() {
                solids.clear();
            }
            for (id, n) in bodies {
                if ui
                    .selectable_label(current.as_deref() == Some(id.as_str()), n)
                    .clicked()
                {
                    *solids = vec![id.clone()];
                }
            }
        });
    ui.end_row();
}

pub(super) fn output_rows(ui: &mut egui::Ui, output: &mut Output, op: &mut Operation) {
    plabel(ui, "Output");
    ui.horizontal(|ui| {
        ui.selectable_value(output, Output::Solid, "Solid");
        ui.selectable_value(output, Output::Surface, "Surface")
            .on_hover_text("Walls only, a new surface body (no operation)");
    });
    ui.end_row();
    plabel(ui, "Operation");
    // A surface is a body of its own: no operation (Inventor greys them).
    ui.add_enabled_ui(*output == Output::Solid, |ui| {
        ui.vertical(|ui| {
            for o in [
                Operation::Join,
                Operation::Cut,
                Operation::Intersect,
                Operation::NewSolid,
            ] {
                ui.radio_value(op, o, o.label());
            }
        });
    });
    ui.end_row();
}

pub(super) fn buttons(
    ui: &mut egui::Ui,
    preview: &Result<Vec<Vec<Point3>>, String>,
    new: bool,
) -> Option<Button> {
    match preview {
        Err(e) if e == super::PREVIEW_PENDING => {
            ui.label(e);
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(50));
        }
        Err(e) if !e.is_empty() => warning_box(ui, e),
        _ => {}
    }
    let ok = preview.is_ok() || matches!(preview, Err(e) if e == super::PREVIEW_PENDING);
    let mut out = None;
    ui.horizontal(|ui| {
        let size = egui::vec2(60.0, 22.0);
        if ui
            .add_enabled(ok, egui::Button::new("OK").min_size(size))
            .on_hover_text("Enter, Ctrl+Enter")
            .clicked()
            || super::keys::panel_ok_key(ui, ok)
        {
            out = Some(Button::Ok);
        }
        if new
            && ui
                .add_enabled(ok, egui::Button::new("Apply").min_size(size))
                .on_hover_text("Create it and start a new one with the same settings")
                .clicked()
        {
            out = Some(Button::Apply);
        }
        if ui.add(egui::Button::new("Cancel").min_size(size)).clicked() {
            out = Some(Button::Cancel);
        }
    });
    out
}

pub(super) fn describe_path(r: &PathRef, part: &crate::part::PartDocument) -> String {
    match r {
        PathRef::Sketch { curves, .. } if curves.is_empty() => "none".into(),
        PathRef::Sketch { sketch, curves } => {
            let name = part
                .sketches
                .iter()
                .find(|s| &s.id == sketch)
                .map_or(sketch.as_str(), |s| s.name.as_str());
            format!("{} curve(s) of {name}", curves.len())
        }
        PathRef::Edges(e) if e.is_empty() => "none".into(),
        PathRef::Edges(e) => format!("{} edge(s)", e.len()),
    }
}

pub(super) fn describe_section(s: &LoftSection, part: &crate::part::PartDocument) -> String {
    let name = |id: &str| {
        part.sketches
            .iter()
            .find(|p| p.id == id)
            .map_or(id.to_owned(), |p| p.name.clone())
    };
    match s {
        LoftSection::Profile { sketch, .. } => format!("Profile of {}", name(sketch)),
        LoftSection::Point { sketch, .. } => format!("Point of {}", name(sketch)),
        LoftSection::FaceLoop(f) => format!("Face {}", f.face),
        LoftSection::Edges(e) => format!(
            "{} edge(s) along {}",
            e.len(),
            e.first().map_or("", |x| x.face.as_str())
        ),
    }
}
