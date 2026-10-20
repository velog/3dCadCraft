//! The Coil and Rib commands (`docs/spec/5.3-coil.md`,
//! `docs/spec/2.7-rib.md`): selector rows (`command_ui::selector_rows`),
//! picks in the graphics window (regions and the axis line for Coil, curves
//! and boss centres for Rib), a preview computed off the frame, OK / Apply
//! / Cancel and Edit Feature.

use eframe::egui::{self, Pos2, Rect};

use super::CadCraftApp;
use super::command_ui::{RowEvents, cursor_hint, selector_rows};
use super::icons::Icon;
use super::panels::{Fields, field_row, plabel, psection, warning_box};
use crate::command::flow::{Flow, PickKind, Selector};
use crate::kernel::Point3;
use crate::part::work::LineRef;
use crate::part::{
    CoilAxis, CoilFeature, CoilMethod, DraftHold, Feature, Operation, Output, ProfileRef,
    RibDirection, RibExtent, RibFeature, RibSide,
};
use crate::sketch::{EntityId, PointId};

/// The open Coil command.
#[derive(Clone, Debug)]
pub(super) struct CoilDraft {
    /// The feature being defined.
    pub def: CoilFeature,
    /// Index of the profile sketch, once a profile is picked.
    pub sketch: Option<usize>,
    /// The active selector (`profiles` / `axis`).
    pub active: &'static str,
    /// Edit Feature of this feature index.
    pub edit: Option<usize>,
    /// The axis has been chosen (else the next pick sets it).
    pub axis_set: bool,
    /// Keep the sketch visible after Apply (+).
    pub keep_sketch_visible: bool,
}

/// The open Rib command.
#[derive(Clone, Debug)]
pub(super) struct RibDraft {
    /// The feature being defined.
    pub def: RibFeature,
    /// Index of the profile sketch.
    pub sketch: Option<usize>,
    /// The active selector (`profile` / `centers`).
    pub active: &'static str,
    /// Edit Feature of this feature index.
    pub edit: Option<usize>,
}

const PICK: egui::Color32 = egui::Color32::from_rgb(60, 190, 90);
const CHOSEN: egui::Color32 = egui::Color32::from_rgb(40, 120, 230);
const PREVIEW: egui::Color32 = egui::Color32::from_rgb(230, 120, 20);

fn coil_flow(d: &CoilDraft) -> Flow {
    Flow::new(
        vec![
            Selector {
                key: "profiles",
                label: "Profiles",
                prompt: "click the profile (a closed region of a sketch)",
                accepts: &[PickKind::Sketch],
                single: false,
                required: true,
                count: d.def.profiles.len(),
                color: [40, 120, 230],
            },
            Selector {
                key: "axis",
                label: "Axis",
                prompt: "click the axis (a line of the sketch or a straight edge), or choose an origin or work axis",
                accepts: &[PickKind::StraightEdge, PickKind::Axis],
                single: true,
                required: true,
                count: usize::from(d.axis_set),
                color: [200, 90, 40],
            },
        ],
        Some(d.active),
    )
}

fn rib_flow(d: &RibDraft) -> Flow {
    let mut s = vec![Selector {
        key: "profile",
        label: "Profile",
        prompt: "click the profile curves (a second click removes one)",
        accepts: &[PickKind::Sketch],
        single: false,
        required: true,
        count: d.def.curves.len(),
        color: [40, 120, 230],
    }];
    if d.def.direction == RibDirection::Normal {
        s.push(Selector {
            key: "centers",
            label: "Boss Centers",
            prompt: "click sketch points on the profile for bosses (optional), or OK",
            accepts: &[PickKind::Sketch],
            single: false,
            required: false,
            count: d.def.boss.centers.len(),
            color: [160, 60, 200],
        });
    }
    Flow::new(s, Some(d.active))
}

/// A pick in the graphics window.
#[derive(Clone, Debug)]
enum Pick {
    Region(usize, ProfileRef),
    Line(EntityId),
    Edge(String),
    Curve(usize, EntityId),
    Point(PointId),
}

impl CadCraftApp {
    /// Whether Coil or Rib is open.
    pub(super) fn coil_rib_open(&self) -> bool {
        self.coil_draft.is_some() || self.rib_draft.is_some()
    }

    /// The last visible sketch (the selected or edited one first) that
    /// `usable` accepts.
    fn pick_sketch(&self, usable: &dyn Fn(&crate::part::PartSketch) -> bool) -> Option<usize> {
        let preferred = self.editing.or(match self.selection {
            super::Selection::Sketch(i) => Some(i),
            _ => None,
        });
        let part = self.part();
        let ok = |i: usize| {
            let ps = &part.sketches[i];
            (ps.visible || part.consumers(&ps.id).is_empty()) && usable(ps)
        };
        preferred
            .filter(|&i| i < part.sketches.len() && ok(i))
            .or_else(|| (0..part.sketches.len()).rev().find(|&i| ok(i)))
    }

    /// **Coil**: the sketch's only region is the profile and its only
    /// centerline the axis (Inventor); otherwise they are picked.
    pub(super) fn start_coil(&mut self) {
        self.finish_sketch();
        self.mode = super::Mode::Normal;
        self.cancel_extrude();
        self.work_draft = None;
        self.blend_draft = None;
        let sketch = self.pick_sketch(&|ps| !ps.sketch.regions().is_empty());
        let mut d = CoilDraft {
            def: CoilFeature::new(Vec::new(), CoilAxis::Line(LineRef::Origin(2))),
            sketch,
            active: "profiles",
            edit: None,
            axis_set: false,
            keep_sketch_visible: false,
        };
        d.def.operation = Operation::default_for(&self.part().body);
        if let Some(i) = sketch {
            let sk = &self.part().sketches[i].sketch;
            let regions = self.sketch_regions(sk);
            if regions.len() == 1 {
                d.def.profiles = vec![ProfileRef::of(&regions[0], regions[0].interior_point())];
                d.active = "axis";
            }
            if let Some(a) = crate::part::coil::auto_axis(sk) {
                d.def.axis = a;
                d.axis_set = true;
            }
        }
        self.coil_draft = Some(d);
    }

    /// **Rib**: the latest sketch with free curves; all its curves form the
    /// profile.
    pub(super) fn start_rib(&mut self) {
        self.finish_sketch();
        self.mode = super::Mode::Normal;
        self.cancel_extrude();
        self.work_draft = None;
        self.blend_draft = None;
        let sketch = self.pick_sketch(&|ps| !crate::part::rib::default_curves(ps).is_empty());
        let curves = sketch
            .map(|i| crate::part::rib::default_curves(&self.part().sketches[i]))
            .unwrap_or_default();
        let active = if curves.is_empty() {
            "profile"
        } else {
            "centers"
        };
        self.rib_draft = Some(RibDraft {
            def: RibFeature::new(curves, 2.0),
            sketch,
            active,
            edit: None,
        });
    }

    /// Edit Feature of a coil.
    pub(super) fn edit_coil(&mut self, def: CoilFeature, k: usize) {
        let sketch = self.part().sketches.iter().position(|s| s.id == def.sketch);
        self.roll_back_to(k);
        self.coil_draft = Some(CoilDraft {
            def,
            sketch,
            active: "profiles",
            edit: Some(k),
            axis_set: true,
            keep_sketch_visible: false,
        });
    }

    /// Edit Feature of a rib.
    pub(super) fn edit_rib(&mut self, def: RibFeature, k: usize) {
        let sketch = self.part().sketches.iter().position(|s| s.id == def.sketch);
        self.roll_back_to(k);
        self.rib_draft = Some(RibDraft {
            def,
            sketch,
            active: "profile",
            edit: Some(k),
        });
    }

    /// The status-bar prompt of the open Coil / Rib.
    pub(super) fn coil_rib_prompt(&self) -> Option<String> {
        let (name, flow) = if let Some(d) = &self.coil_draft {
            ("Coil", coil_flow(d))
        } else if let Some(d) = &self.rib_draft {
            ("Rib", rib_flow(d))
        } else {
            return None;
        };
        let ok = if flow.ready() {
            super::i18n::tr(super::command_ui::OR_OK)
        } else {
            "".into()
        };
        Some(format!("{name}: {}{ok}", super::i18n::tr(flow.prompt())))
    }

    /// Picks, highlights and the preview in the graphics window.
    pub(super) fn coil_rib_view(
        &mut self,
        painter: &egui::Painter,
        rect: Rect,
        pointer: Option<Pos2>,
        clicked: bool,
    ) {
        if !self.coil_rib_open() {
            return;
        }
        for pts in self.coil_rib_chosen() {
            self.draw_visible_polyline(painter, rect, &pts, egui::Stroke::new(3.0, CHOSEN));
        }
        if let Some(p) = pointer {
            let hover = self.coil_rib_hover(p, rect);
            if let Some((_, lines)) = &hover {
                for line in lines {
                    self.draw_visible_polyline(painter, rect, line, egui::Stroke::new(3.0, PICK));
                }
            }
            if let Some(text) = self.coil_rib_prompt() {
                cursor_hint(painter, rect, p, &text);
            }
            if clicked && let Some((pick, _)) = hover {
                self.coil_rib_pick(pick);
            }
        }
        let preview = if self.coil_draft.is_some() {
            self.coil_preview()
        } else {
            self.rib_preview()
        };
        if let Ok(edges) = preview {
            let stroke = egui::Stroke::new(2.0, PREVIEW);
            for e in edges {
                let pts: Vec<_> = e
                    .iter()
                    .filter_map(|p| self.camera.project(p, rect))
                    .collect();
                painter.add(egui::Shape::line(pts, stroke));
            }
        }
    }

    /// The chosen profiles / axis / curves as polylines.
    fn coil_rib_chosen(&self) -> Vec<Vec<Point3>> {
        let part = self.part();
        let mut out = Vec::new();
        if let Some(d) = &self.coil_draft
            && let Some(ps) = d.sketch.and_then(|i| part.sketches.get(i))
        {
            let regions = self.sketch_regions(&ps.sketch);
            let pl = &ps.sketch.plane;
            for pr in &d.def.profiles {
                if let Some(r) = pr.resolve(&regions) {
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
            if d.axis_set
                && let Ok((o, k)) = d.def.world_axis(&ps.sketch, &part.body, &part.work)
            {
                let (_, _, h) = d.def.sizes().unwrap_or((0.0, 0.0, 20.0));
                let l = h.abs().max(20.0);
                out.push(vec![o - k * (0.2 * l), o + k * (1.2 * l)]);
            }
        }
        if let Some(d) = &self.rib_draft
            && let Some(ps) = d.sketch.and_then(|i| part.sketches.get(i))
            && let Ok(chains) = crate::part::rib::chains(&ps.sketch, &d.def.curves)
        {
            let pl = &ps.sketch.plane;
            for c in chains {
                let mut pts: Vec<Point3> = c.points.iter().map(|q| pl.point_at(q.x, q.y)).collect();
                if c.closed
                    && let Some(f) = pts.first().copied()
                {
                    pts.push(f);
                }
                out.push(pts);
            }
            for &pt in &d.def.boss.centers {
                if let Some(q) = ps.sketch.point(pt) {
                    let w = pl.point_at(q.position.x, q.position.y);
                    let r = d.def.boss.diameter / 2.0;
                    let ring: Vec<Point3> = (0..=32)
                        .map(|k| {
                            let t = std::f64::consts::TAU * k as f64 / 32.0;
                            w + pl.x_axis.into_inner() * (r * t.cos())
                                + pl.y_axis().into_inner() * (r * t.sin())
                        })
                        .collect();
                    out.push(ring);
                }
            }
        }
        out
    }

    /// What a click at `p` would pick, with its outline.
    fn coil_rib_hover(&self, p: Pos2, rect: Rect) -> Option<(Pick, Vec<Vec<Point3>>)> {
        use super::overlay;
        let part = self.part();
        if let Some(d) = &self.coil_draft {
            if d.active == "axis" {
                // A line of the profile sketch, else a straight edge.
                if let Some(ps) = d.sketch.and_then(|i| part.sketches.get(i))
                    && let Some((e, _)) = overlay::pick_entity(
                        &self.camera,
                        rect,
                        &ps.sketch,
                        p,
                        overlay::CURVE_SNAP_PX,
                    )
                    && let Some(crate::sketch::Entity::Line { start, end }) = ps.sketch.entity(e)
                    && let (Some(a), Some(b)) = (ps.sketch.point(*start), ps.sketch.point(*end))
                {
                    let pl = &ps.sketch.plane;
                    let line = vec![
                        pl.point_at(a.position.x, a.position.y),
                        pl.point_at(b.position.x, b.position.y),
                    ];
                    return Some((Pick::Line(e), vec![line]));
                }
                let e = self.edge_under(p, rect)?;
                part.body.edge_direction(e)?;
                let name = part.body.edge(e)?.name.clone()?;
                let pts = part
                    .body
                    .sample_edge(e, &crate::kernel::TessellationOptions::default())
                    .ok()?;
                return Some((Pick::Edge(name), vec![pts]));
            }
            for (i, ps) in part.sketches.iter().enumerate().filter(|(_, s)| s.visible) {
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
            return None;
        }
        let d = self.rib_draft.as_ref()?;
        if d.active == "centers" {
            let ps = d.sketch.and_then(|i| part.sketches.get(i))?;
            let pt =
                overlay::pick_any_point(&self.camera, rect, &ps.sketch, p, overlay::POINT_PICK_PX)?;
            let q = ps.sketch.point(pt)?;
            let w = ps.sketch.plane.point_at(q.position.x, q.position.y);
            return Some((Pick::Point(pt), vec![vec![w, w]]));
        }
        for (i, ps) in part.sketches.iter().enumerate().filter(|(_, s)| s.visible) {
            if d.sketch.is_some() && d.sketch != Some(i) && !d.def.curves.is_empty() {
                continue;
            }
            if let Some((e, _)) =
                overlay::pick_entity(&self.camera, rect, &ps.sketch, p, overlay::CURVE_SNAP_PX)
                && let Ok(ch) = crate::part::rib::chains(&ps.sketch, &[e])
            {
                let pl = &ps.sketch.plane;
                let lines = ch
                    .into_iter()
                    .map(|c| c.points.iter().map(|q| pl.point_at(q.x, q.y)).collect())
                    .collect();
                return Some((Pick::Curve(i, e), lines));
            }
        }
        None
    }

    /// Applies a pick to the active selector (single-pick selectors move on).
    fn coil_rib_pick(&mut self, pick: Pick) {
        if let Some(d) = self.coil_draft.as_mut() {
            match pick {
                Pick::Region(i, pr) => {
                    if d.sketch != Some(i) {
                        d.sketch = Some(i);
                        d.def.profiles = vec![pr];
                    } else if let Some(k) =
                        d.def.profiles.iter().position(|p| p.sources == pr.sources)
                    {
                        d.def.profiles.remove(k);
                    } else {
                        d.def.profiles.push(pr);
                    }
                    if !d.axis_set && !d.def.profiles.is_empty() {
                        d.active = "axis";
                    }
                }
                Pick::Line(e) => {
                    d.def.axis = CoilAxis::Sketch(e);
                    d.axis_set = true;
                    d.active = "profiles";
                }
                Pick::Edge(name) => {
                    d.def.axis = CoilAxis::Line(LineRef::Edge(name));
                    d.axis_set = true;
                    d.active = "profiles";
                }
                Pick::Curve(..) | Pick::Point(_) => {}
            }
            return;
        }
        let Some(d) = self.rib_draft.as_mut() else {
            return;
        };
        match pick {
            Pick::Curve(i, e) => {
                if d.sketch != Some(i) {
                    d.sketch = Some(i);
                    d.def.curves = vec![e];
                    d.def.boss.centers.clear();
                } else if let Some(k) = d.def.curves.iter().position(|&c| c == e) {
                    d.def.curves.remove(k);
                } else {
                    d.def.curves.push(e);
                }
            }
            Pick::Point(pt) => {
                if let Some(k) = d.def.boss.centers.iter().position(|&c| c == pt) {
                    d.def.boss.centers.remove(k);
                } else {
                    d.def.boss.centers.push(pt);
                }
            }
            _ => {}
        }
    }

    /// The coil's result as edge polylines (off the frame), or why not.
    pub(super) fn coil_preview(&self) -> Result<Vec<Vec<Point3>>, String> {
        let Some(d) = &self.coil_draft else {
            return Err(String::new());
        };
        let Some(sketch) = d.sketch.filter(|_| !d.def.profiles.is_empty()) else {
            return Err("Select a profile".into());
        };
        if !d.axis_set {
            return Err("Select the axis".into());
        }
        let key = format!(
            "coil|{:?}|{sketch}|{}",
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
                Some(k) => trial.replace_feature(k, Feature::Coil(def)),
                None => trial.add_coil(sketch, def),
            };
            built
                .map(|id| Self::feature_polylines(&trial, &id))
                .map_err(|e| e.to_string())
        })
    }

    /// The rib's result as edge polylines (off the frame), or why not.
    pub(super) fn rib_preview(&self) -> Result<Vec<Vec<Point3>>, String> {
        let Some(d) = &self.rib_draft else {
            return Err(String::new());
        };
        let Some(sketch) = d.sketch.filter(|_| !d.def.curves.is_empty()) else {
            return Err("Select the profile curves".into());
        };
        let key = format!(
            "rib|{:?}|{sketch}|{}",
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
                Some(k) => trial.replace_feature(k, Feature::Rib(def)),
                None => trial.add_rib(sketch, def),
            };
            built
                .map(|id| Self::feature_polylines(&trial, &id))
                .map_err(|e| e.to_string())
        })
    }

    /// OK (`keep_open` false) or Apply (+) of the Coil.
    pub(super) fn commit_coil(&mut self, keep_open: bool) {
        let Some(d) = self.coil_draft.clone() else {
            return;
        };
        let Some(sketch) = d.sketch else {
            self.file_status = Some(Err("Coil: select a profile".into()));
            return;
        };
        let cmd = if d.edit.is_some() {
            "Edit Feature"
        } else {
            "Coil"
        };
        self.checkpoint(cmd);
        let built = match d.edit {
            Some(k) => self
                .part_mut()
                .replace_feature(k, Feature::Coil(d.def.clone())),
            None => self.part_mut().add_coil(sketch, d.def.clone()),
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
                    // Apply (+): the sketch stays visible when asked.
                    if d.keep_sketch_visible
                        && let Some(ps) = self.part_mut().sketches.get_mut(sketch)
                    {
                        ps.visible = true;
                    }
                    let mut next = d;
                    next.def.profiles.clear();
                    next.active = "profiles";
                    self.coil_draft = Some(next);
                } else {
                    self.coil_draft = None;
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

    /// OK (`keep_open` false) or Apply of the Rib.
    pub(super) fn commit_rib(&mut self, keep_open: bool) {
        let Some(d) = self.rib_draft.clone() else {
            return;
        };
        let Some(sketch) = d.sketch else {
            self.file_status = Some(Err("Rib: select the profile curves".into()));
            return;
        };
        let cmd = if d.edit.is_some() {
            "Edit Feature"
        } else {
            "Rib"
        };
        self.checkpoint(cmd);
        let built = match d.edit {
            Some(k) => self
                .part_mut()
                .replace_feature(k, Feature::Rib(d.def.clone())),
            None => self.part_mut().add_rib(sketch, d.def.clone()),
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
                    next.def.curves.clear();
                    next.def.boss.centers.clear();
                    next.sketch = None;
                    next.active = "profile";
                    self.rib_draft = Some(next);
                } else {
                    self.rib_draft = None;
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

    /// Applies typed field values (expressions) to a draft feature.
    fn apply_fields(&mut self, f: &mut Feature, commits: Vec<(&'static str, String)>, cmd: &str) {
        for (field, text) in commits {
            match self.part().apply_draft_field(f, field, &text) {
                Ok(()) => self.file_status = None,
                Err(e) => self.file_status = Some(Err(format!("{cmd}: {e}"))),
            }
        }
    }

    /// The Coil panel (Coil Shape, Coil Size, Coil Ends).
    pub(super) fn coil_dialog(&mut self, ui: &mut egui::Ui) {
        let Some(mut d) = self.coil_draft.clone() else {
            return;
        };
        self.panel_header(ui, Icon::Coil, "Coil");
        let part = self.part();
        let bodies = part.bodies_before(d.edit.unwrap_or(part.insert_index()));
        let axis_text = if d.axis_set {
            describe_axis(&d.def.axis)
        } else {
            String::new()
        };
        let mut ev = RowEvents::default();
        psection(ui, "coil_input", "Input Geometry", |ui| {
            ev = selector_rows(
                ui,
                &coil_flow(&d),
                &[
                    ("profiles", format!("{} selected", d.def.profiles.len())),
                    ("axis", axis_text.clone()),
                ],
            );
            plabel(ui, "Axis");
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("coil_axis_list")
                    .selected_text("Origin / work axis")
                    .show_ui(ui, |ui| {
                        for (k, n) in ["X Axis", "Y Axis", "Z Axis"].iter().enumerate() {
                            if ui.selectable_label(false, *n).clicked() {
                                d.def.axis = CoilAxis::Line(LineRef::Origin(k));
                                d.axis_set = true;
                            }
                        }
                        let mut axes: Vec<&String> = part.work.axes.keys().collect();
                        axes.sort();
                        for id in axes {
                            if ui.selectable_label(false, id).clicked() {
                                d.def.axis = CoilAxis::Line(LineRef::Work(id.clone()));
                                d.axis_set = true;
                            }
                        }
                    });
                ui.toggle_value(&mut d.def.flip, "Flip")
                    .on_hover_text("The coil climbs the other way along the axis");
            });
            ui.end_row();
            if bodies.len() > 1 {
                solids_row(ui, &bodies, &mut d.def.solids);
            }
        });
        if let Some(k) = ev.activate {
            d.active = if k == "axis" { "axis" } else { "profiles" };
        }
        if let Some(k) = ev.clear {
            if k == "axis" {
                d.axis_set = false;
                d.active = "axis";
            } else {
                d.def.profiles.clear();
                d.active = "profiles";
            }
        }
        let table = self.part().parameter_table();
        let mut fx = Fields {
            table: &table,
            key: "coil_draft".into(),
            commits: Vec::new(),
            bodies: Vec::new(),
        };
        let snapshot = Feature::Coil(d.def.clone());
        psection(ui, "coil_size", "Behavior", |ui| {
            plabel(ui, "Method");
            egui::ComboBox::from_id_salt("coil_method")
                .selected_text(d.def.method.label())
                .show_ui(ui, |ui| {
                    for m in CoilMethod::ALL {
                        ui.selectable_value(&mut d.def.method, m, m.label());
                    }
                });
            ui.end_row();
            match d.def.method {
                CoilMethod::PitchAndRevolution => {
                    field_row(ui, &mut fx, "Pitch", &snapshot, "pitch");
                    field_row(ui, &mut fx, "Revolution", &snapshot, "revolutions");
                }
                CoilMethod::RevolutionAndHeight => {
                    field_row(ui, &mut fx, "Revolution", &snapshot, "revolutions");
                    field_row(ui, &mut fx, "Height", &snapshot, "height");
                }
                CoilMethod::PitchAndHeight => {
                    field_row(ui, &mut fx, "Pitch", &snapshot, "pitch");
                    field_row(ui, &mut fx, "Height", &snapshot, "height");
                }
            }
            field_row(ui, &mut fx, "Taper", &snapshot, "taper");
            plabel(ui, "Rotation");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut d.def.left_hand, false, "Right")
                    .on_hover_text("Right-hand coil (counter-clockwise about the axis)");
                ui.selectable_value(&mut d.def.left_hand, true, "Left");
            });
            ui.end_row();
            if let Ok((p, t, h)) = d.def.sizes() {
                plabel(ui, "Result");
                ui.label(format!(
                    "pitch {p:.3} mm, {:.3} rev, height {h:.3} mm",
                    t / std::f64::consts::TAU
                ));
                ui.end_row();
            }
        });
        psection(ui, "coil_ends", "Coil Ends", |ui| {
            plabel(ui, "Start");
            ui.checkbox(&mut d.def.start.closed, "Close Start")
                .on_hover_text("The pitch runs down to zero at the start (both off: natural ends)");
            ui.end_row();
            if d.def.start.closed {
                field_row(
                    ui,
                    &mut fx,
                    "Transition Angle",
                    &snapshot,
                    "start_transition",
                );
                field_row(ui, &mut fx, "Flat Angle", &snapshot, "start_flat");
            }
            plabel(ui, "End");
            ui.checkbox(&mut d.def.end.closed, "Close End");
            ui.end_row();
            if d.def.end.closed {
                field_row(ui, &mut fx, "Transition Angle", &snapshot, "end_transition");
                field_row(ui, &mut fx, "Flat Angle", &snapshot, "end_flat");
            }
        });
        let commits = std::mem::take(&mut fx.commits);
        drop(fx);
        let mut f = Feature::Coil(d.def.clone());
        self.apply_fields(&mut f, commits, "Coil");
        if let Feature::Coil(c) = f {
            d.def = c;
        }
        psection(ui, "coil_output", "Output", |ui| {
            plabel(ui, "Output");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut d.def.output, Output::Solid, "Solid");
                ui.selectable_value(&mut d.def.output, Output::Surface, "Surface")
                    .on_hover_text("The coil's walls only (the part is then built faceted)");
            });
            ui.end_row();
            if d.def.output == Output::Solid {
                plabel(ui, "Operation");
                ui.vertical(|ui| {
                    for o in [
                        Operation::Join,
                        Operation::Cut,
                        Operation::Intersect,
                        Operation::NewSolid,
                    ] {
                        ui.radio_value(&mut d.def.operation, o, o.label());
                    }
                });
                ui.end_row();
            }
            plabel(ui, "Apply (+)");
            ui.checkbox(&mut d.keep_sketch_visible, "Keep sketch visible")
                .on_hover_text("After Apply the sketch stays visible for the next coil");
            ui.end_row();
        });
        self.coil_draft = Some(d.clone());
        let preview = self.coil_preview();
        match buttons(ui, &preview, d.edit.is_none()) {
            Some(Button::Ok) => self.commit_coil(false),
            Some(Button::Apply) => self.commit_coil(true),
            Some(Button::Cancel) => {
                self.coil_draft = None;
                self.end_feature_edit();
            }
            None => {}
        }
    }

    /// The Rib panel (Shape, Draft, Boss).
    pub(super) fn rib_dialog(&mut self, ui: &mut egui::Ui) {
        let Some(mut d) = self.rib_draft.clone() else {
            return;
        };
        self.panel_header(ui, Icon::Rib, "Rib");
        let part = self.part();
        let bodies = part.bodies_before(d.edit.unwrap_or(part.insert_index()));
        // Extend Profile is offered only when an end stops short of the body.
        let short = d
            .sketch
            .and_then(|i| part.sketches.get(i))
            .is_some_and(|ps| !d.def.ends_reach(&ps.sketch, &part.body));
        let mut ev = RowEvents::default();
        psection(ui, "rib_input", "Shape", |ui| {
            ev = selector_rows(
                ui,
                &rib_flow(&d),
                &[
                    ("profile", format!("{} curve(s)", d.def.curves.len())),
                    ("centers", format!("{} point(s)", d.def.boss.centers.len())),
                ],
            );
            plabel(ui, "Direction");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut d.def.direction, RibDirection::Normal, "Normal")
                    .on_hover_text("Normal to Sketch Plane (a web): drawn along the sketch normal");
                ui.selectable_value(&mut d.def.direction, RibDirection::Parallel, "Parallel")
                    .on_hover_text("Parallel to Sketch Plane (a rib): spreads in the sketch plane");
                ui.toggle_value(&mut d.def.flip, "Flip")
                    .on_hover_text("Grow away from the side the body is on");
            });
            ui.end_row();
            if bodies.len() > 1 {
                solids_row(ui, &bodies, &mut d.def.solids);
            }
            if short {
                plabel(ui, "Extend Profile");
                ui.checkbox(&mut d.def.extend, "").on_hover_text(
                    "The profile's ends run on to the body (off: exactly the profile)",
                );
                ui.end_row();
            }
        });
        if let Some(k) = ev.activate {
            d.active = if k == "centers" { "centers" } else { "profile" };
        }
        if let Some(k) = ev.clear {
            if k == "centers" {
                d.def.boss.centers.clear();
            } else {
                d.def.curves.clear();
                d.active = "profile";
            }
        }
        let table = self.part().parameter_table();
        let mut fx = Fields {
            table: &table,
            key: "rib_draft".into(),
            commits: Vec::new(),
            bodies: Vec::new(),
        };
        let snapshot = Feature::Rib(d.def.clone());
        let normal = d.def.direction == RibDirection::Normal;
        psection(ui, "rib_size", "Thickness and Depth", |ui| {
            field_row(ui, &mut fx, "Thickness", &snapshot, "thickness");
            plabel(ui, "Side");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut d.def.side, RibSide::Side1, "Side 1");
                ui.selectable_value(&mut d.def.side, RibSide::Symmetric, "Symmetric");
                ui.selectable_value(&mut d.def.side, RibSide::Side2, "Side 2");
            });
            ui.end_row();
            plabel(ui, "Depth");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut d.def.extent, RibExtent::ToNext, "To Next");
                ui.selectable_value(&mut d.def.extent, RibExtent::Finite, "Finite");
            });
            ui.end_row();
            if d.def.extent == RibExtent::Finite {
                field_row(ui, &mut fx, "Distance", &snapshot, "depth");
            }
            if normal {
                field_row(ui, &mut fx, "Taper", &snapshot, "taper");
            }
        });
        if normal {
            psection(ui, "rib_draft", "Draft", |ui| {
                plabel(ui, "Hold Thickness");
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut d.def.hold, DraftHold::Top, "At Top");
                    ui.selectable_value(&mut d.def.hold, DraftHold::Root, "At Root");
                });
                ui.end_row();
                field_row(ui, &mut fx, "Draft Angle", &snapshot, "draft");
            });
            psection(ui, "rib_boss", "Boss", |ui| {
                plabel(ui, "Centers");
                if ui
                    .button("Select All")
                    .on_hover_text("Every sketch point on the profile")
                    .clicked()
                    && let Some(ps) = d.sketch.and_then(|i| self.part().sketches.get(i))
                {
                    d.def.boss.centers = points_on_profile(ps, &d.def.curves);
                }
                ui.end_row();
                if !d.def.boss.centers.is_empty() {
                    field_row(ui, &mut fx, "Diameter", &snapshot, "boss_diameter");
                    field_row(ui, &mut fx, "Offset", &snapshot, "boss_offset");
                    field_row(ui, &mut fx, "Draft Angle", &snapshot, "boss_draft");
                }
            });
        }
        let commits = std::mem::take(&mut fx.commits);
        drop(fx);
        let mut f = Feature::Rib(d.def.clone());
        self.apply_fields(&mut f, commits, "Rib");
        if let Feature::Rib(r) = f {
            d.def = r;
        }
        self.rib_draft = Some(d.clone());
        let preview = self.rib_preview();
        match buttons(ui, &preview, d.edit.is_none()) {
            Some(Button::Ok) => self.commit_rib(false),
            Some(Button::Apply) => self.commit_rib(true),
            Some(Button::Cancel) => {
                self.rib_draft = None;
                self.end_feature_edit();
            }
            None => {}
        }
    }

    /// Properties of a Coil in the browser selection.
    pub(super) fn coil_properties(&mut self, ui: &mut egui::Ui, k: usize, c: CoilFeature) {
        self.panel_header(ui, Icon::Coil, &c.name);
        if let Some(h) = self.part().feature_health.get(&c.id) {
            warning_box(ui, h);
        }
        let sizes = c.sizes();
        psection(ui, &format!("coil_props_{k}"), "Coil", |ui| {
            plabel(ui, "Profiles");
            ui.label(format!("{}", c.profiles.len()));
            ui.end_row();
            plabel(ui, "Axis");
            ui.label(describe_axis(&c.axis));
            ui.end_row();
            plabel(ui, "Method");
            ui.label(c.method.label());
            ui.end_row();
            if let Ok((p, t, h)) = sizes {
                plabel(ui, "Size");
                ui.label(format!(
                    "pitch {p:.3} mm, {:.3} rev, height {h:.3} mm",
                    t / std::f64::consts::TAU
                ));
                ui.end_row();
            }
            plabel(ui, "Rotation");
            ui.label(if c.left_hand { "Left" } else { "Right" });
            ui.end_row();
            plabel(ui, "Operation");
            ui.label(c.operation.label());
            ui.end_row();
        });
        if ui.button("Edit Feature").clicked() {
            self.edit_feature_dialog(k);
        }
    }

    /// Properties of a Rib in the browser selection.
    pub(super) fn rib_properties(&mut self, ui: &mut egui::Ui, k: usize, r: RibFeature) {
        self.panel_header(ui, Icon::Rib, &r.name);
        if let Some(h) = self.part().feature_health.get(&r.id) {
            warning_box(ui, h);
        }
        psection(ui, &format!("rib_props_{k}"), "Rib", |ui| {
            plabel(ui, "Curves");
            ui.label(format!("{}", r.curves.len()));
            ui.end_row();
            plabel(ui, "Direction");
            ui.label(match r.direction {
                RibDirection::Normal => "Normal to Sketch Plane",
                RibDirection::Parallel => "Parallel to Sketch Plane",
            });
            ui.end_row();
            plabel(ui, "Thickness");
            ui.label(format!("{:.3} mm", r.thickness));
            ui.end_row();
            plabel(ui, "Depth");
            ui.label(match r.extent {
                RibExtent::ToNext => "To Next".to_owned(),
                RibExtent::Finite => format!("{:.3} mm", r.depth),
            });
            ui.end_row();
            if !r.boss.centers.is_empty() {
                plabel(ui, "Bosses");
                ui.label(format!(
                    "{} × Ø{:.3}",
                    r.boss.centers.len(),
                    r.boss.diameter
                ));
                ui.end_row();
            }
        });
        if ui.button("Edit Feature").clicked() {
            self.edit_feature_dialog(k);
        }
    }
}

/// Every free sketch point (not a curve's own point) on the profile curves.
fn points_on_profile(ps: &crate::part::PartSketch, curves: &[EntityId]) -> Vec<PointId> {
    let sk = &ps.sketch;
    let Ok(chains) = crate::part::rib::chains(sk, curves) else {
        return Vec::new();
    };
    let used: Vec<PointId> = sk.entities().flat_map(|(_, e)| e.points()).collect();
    sk.points()
        .filter(|(id, _)| !used.contains(id) && !ps.is_projected(*id))
        .filter(|(_, p)| {
            chains.iter().any(|c| {
                let n = c.points.len();
                let segs = if c.closed { n } else { n.saturating_sub(1) };
                (0..segs).any(|i| {
                    let (a, b) = (c.points[i], c.points[(i + 1) % n]);
                    let d = b - a;
                    let l2 = d.norm_squared();
                    let s = if l2 > 0.0 {
                        ((p.position - a).dot(&d) / l2).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    (a + d * s - p.position).norm() < 1e-3
                })
            })
        })
        .map(|(id, _)| id)
        .collect()
}

fn describe_axis(a: &CoilAxis) -> String {
    match a {
        CoilAxis::Sketch(_) => "Sketch line".into(),
        CoilAxis::Line(LineRef::Origin(k)) => ["X Axis", "Y Axis", "Z Axis"]
            .get(*k)
            .copied()
            .unwrap_or("Axis")
            .into(),
        CoilAxis::Line(l) => l.describe(),
    }
}

#[derive(Clone, Copy)]
enum Button {
    Ok,
    Apply,
    Cancel,
}

fn solids_row(ui: &mut egui::Ui, bodies: &[(String, String)], solids: &mut Vec<String>) {
    plabel(ui, "Solids");
    let current = solids.first().cloned();
    let shown = current
        .as_ref()
        .and_then(|c| bodies.iter().find(|(id, _)| id == c))
        .map_or("Default".to_owned(), |(_, n)| n.clone());
    egui::ComboBox::from_id_salt(("coil_rib_solids", bodies.len()))
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

fn buttons(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::part::{OriginPlane, PartDocument, SketchOptions, SketchPlacement};
    use crate::sketch::Point2;

    #[test]
    fn coil_flow_needs_profile_and_axis() {
        let mut d = CoilDraft {
            def: CoilFeature::new(Vec::new(), CoilAxis::Line(LineRef::Origin(2))),
            sketch: None,
            active: "profiles",
            edit: None,
            axis_set: false,
            keep_sketch_visible: false,
        };
        let f = coil_flow(&d);
        assert!(!f.ready());
        assert_eq!(f.active_key(), Some("profiles"));
        d.def.profiles.push(ProfileRef {
            sources: Vec::new(),
            seed: Point2::new(0.0, 0.0),
        });
        d.axis_set = true;
        assert!(coil_flow(&d).ready());
    }

    #[test]
    fn rib_flow_and_boss_points_on_the_profile() {
        let mut part = PartDocument::default();
        let k = part
            .create_sketch(
                SketchPlacement::on_origin_plane(OriginPlane::XY),
                &SketchOptions::default(),
            )
            .unwrap();
        let sk = &mut part.sketches[k].sketch;
        let l = sk.add_line(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0));
        let on = sk.add_point(Point2::new(4.0, 0.0));
        let _off = sk.add_point(Point2::new(4.0, 3.0));
        let pts = points_on_profile(&part.sketches[k], &[l]);
        assert_eq!(pts, vec![on]);
        let mut d = RibDraft {
            def: RibFeature::new(vec![l], 2.0),
            sketch: Some(k),
            active: "profile",
            edit: None,
        };
        assert!(rib_flow(&d).ready(), "bosses are optional");
        d.def.direction = RibDirection::Parallel;
        assert_eq!(
            rib_flow(&d).selectors.len(),
            1,
            "no bosses on a parallel rib"
        );
    }
}
