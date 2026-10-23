//! Extrude / Revolve command extras from the Inventor 2027 comparison
//! (`docs/spec/2.1-extrude.md`, `docs/spec/2.2-revolve.md`): the
//! breadcrumb, the Advanced Settings menu (Predict Boolean Operation, Keep
//! sketch visible on (+)), Apply (+), Ctrl+Enter, and the Primitives
//! (Box, Cylinder, Sphere, Torus) click workflow.

use eframe::egui;

use super::breadcrumb::{self, Crumb, CrumbEvent};
use super::{CadCraftApp, DraftDef, ExtrudeDraft, Mode, PickPurpose, Selection};
use crate::part::{PlaneRef, PrimitiveKind, SketchOptions, SketchPlacement};
use crate::sketch::{Point2, Sketch};

/// The Advanced Settings of the feature panels (Inventor: the menu next to
/// the panel title). Kept for the session.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct FeatureSettings {
    /// **Predict Boolean Operation**: the command proposes Join / Cut from
    /// where the profile and the direction are (until the user picks one).
    pub predict_boolean: bool,
    /// **Keep sketch visible on (+)**: after Apply (+) the sketch stays
    /// visible, so its other regions can be used by the next feature (it
    /// becomes shared).
    pub keep_sketch_visible: bool,
}

impl Default for FeatureSettings {
    fn default() -> Self {
        Self {
            predict_boolean: true,
            keep_sketch_visible: true,
        }
    }
}

/// A primitive being placed: its sketch and the clicks so far.
#[derive(Clone, Debug)]
pub(super) struct PrimDraft {
    /// Which primitive.
    pub kind: PrimitiveKind,
    /// Its (temporary) sketch.
    pub sketch: usize,
    /// Clicks in sketch coordinates.
    pub clicks: Vec<Point2>,
}

/// A cheap fingerprint of a sketch's geometry (preview keys).
pub(super) fn sketch_sig(sk: &Sketch) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for (_, p) in sk.points() {
        p.position.x.to_bits().hash(&mut h);
        p.position.y.to_bits().hash(&mut h);
    }
    sk.entities().count().hash(&mut h);
    h.finish()
}

/// The operation a draft would get from Predict Boolean Operation.
pub(super) fn predicted(
    part: &crate::part::PartDocument,
    d: &ExtrudeDraft,
) -> Option<crate::part::Operation> {
    let ps = part.sketches.get(d.sketch)?;
    Some(match &d.def {
        DraftDef::Extrude(e) => e.predict_operation(&part.body, &ps.sketch),
        DraftDef::Revolve(r) => r.predict_operation(&part.body, &ps.sketch),
    })
}

impl CadCraftApp {
    /// Predict Boolean Operation, each frame of an open draft: while the
    /// user has not chosen an operation, it follows the profile and the
    /// direction (computed again only when they change).
    pub(super) fn predict_draft_operation(&mut self) {
        if !self.feature_settings.predict_boolean {
            return;
        }
        let Some(d) = self.extrude.as_ref() else {
            return;
        };
        if !d.op_auto || d.edit.is_some() {
            return;
        }
        let key = {
            let sig = self
                .part()
                .sketches
                .get(d.sketch)
                .map_or(0, |ps| sketch_sig(&ps.sketch));
            match &d.def {
                DraftDef::Extrude(e) => format!(
                    "{sig}|{:?}|{:?}|{:?}|{}",
                    e.profiles, e.direction, e.extent, e.distance
                ),
                DraftDef::Revolve(r) => format!("{sig}|{:?}", r.profiles),
            }
        };
        if d.predicted_for.as_deref() == Some(key.as_str()) {
            return;
        }
        let op = predicted(self.part(), d);
        if let Some(d) = self.extrude.as_mut() {
            d.predicted_for = Some(key);
            if let Some(op) = op {
                match &mut d.def {
                    DraftDef::Extrude(e) => e.operation = op,
                    DraftDef::Revolve(r) => r.operation = op,
                }
                // Not the user's choice: no automatic flip for it.
                d.last_op = Some(op);
            }
        }
    }

    /// The breadcrumb of the Extrude / Revolve panel (`Sketch1 >
    /// Extrusion1`).
    pub(super) fn feature_breadcrumb(&mut self, ui: &mut egui::Ui) {
        let Some(d) = self.extrude.as_ref() else {
            return;
        };
        let sketch = self
            .part()
            .sketches
            .get(d.sketch)
            .map_or_else(String::new, |s| s.name.clone());
        let name = match &d.def {
            DraftDef::Extrude(e) if !e.name.is_empty() => e.name.clone(),
            DraftDef::Revolve(r) if !r.name.is_empty() => r.name.clone(),
            DraftDef::Extrude(_) => self.part().next_feature_id("Extrusion"),
            DraftDef::Revolve(_) => self.part().next_feature_id("Revolution"),
        };
        let crumbs = [Crumb::link(sketch), Crumb::feature(name)];
        let Some(d) = self.extrude.as_mut() else {
            return;
        };
        let ev = breadcrumb::show(ui, &mut d.crumbs, &crumbs);
        match ev {
            Some(CrumbEvent::Open(_)) => self.edit_draft_sketch(),
            Some(CrumbEvent::Rename(_, n)) => {
                if let Some(d) = self.extrude.as_mut() {
                    match &mut d.def {
                        DraftDef::Extrude(e) => e.name = n,
                        DraftDef::Revolve(r) => r.name = n,
                    }
                }
            }
            Some(CrumbEvent::Back(_)) | None => {}
        }
    }

    /// Breadcrumb → sketch: the command waits (its inputs kept) while the
    /// sketch is edited; Finish Sketch brings it back.
    pub(super) fn edit_draft_sketch(&mut self) {
        let Some(d) = self.extrude.take() else { return };
        let i = d.sketch;
        self.target_pick = None;
        self.suspended_draft = Some(d);
        self.edit_sketch(i);
    }

    /// Finish Sketch after a breadcrumb jump: the command again.
    pub(super) fn resume_suspended_draft(&mut self) {
        if let Some(d) = self.suspended_draft.take() {
            self.extrude = Some(d);
            self.scene_dirty = true;
        }
    }

    /// The Advanced Settings menu of the panel header.
    pub(super) fn feature_advanced_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("...", |ui| {
            let s = &mut self.feature_settings;
            ui.checkbox(&mut s.predict_boolean, "Predict Boolean Operation")
                .on_hover_text("Propose Join or Cut from where the profile and the direction are");
            ui.checkbox(&mut s.keep_sketch_visible, "Keep sketch visible on (+)")
                .on_hover_text("After Apply (+) the sketch stays visible for the next feature");
        })
        .response
        .on_hover_text("Advanced Settings");
    }

    /// Apply (+): creates the feature and starts the command again (on the
    /// same sketch, kept visible when the setting says so).
    pub(super) fn apply_and_continue(&mut self) {
        let Some(d) = self.extrude.as_ref() else {
            return;
        };
        let (sketch, revolve, primitive) =
            (d.sketch, matches!(d.def, DraftDef::Revolve(_)), d.primitive);
        let before = self.part().features.len();
        self.commit_extrude();
        if self.extrude.is_some() || self.part().features.len() == before {
            return; // failed: the dialog stays
        }
        if let Some(kind) = primitive {
            self.start_primitive(kind);
            return;
        }
        if self.feature_settings.keep_sketch_visible
            && let Some(ps) = self.part_mut().sketches.get_mut(sketch)
        {
            ps.visible = true;
        }
        self.selection = Selection::Sketch(sketch);
        self.start_feature(revolve);
    }

    /// Extrude: a click on an open curve of the profile sketch (one that
    /// bounds no region) adds it to the open profile, or removes it again.
    /// Returns whether it was such a click.
    pub(super) fn open_curve_pick(&mut self, pos: egui::Pos2, rect: egui::Rect) -> bool {
        let Some(d) = self.extrude.as_ref() else {
            return false;
        };
        if !matches!(d.def, DraftDef::Extrude(_)) {
            return false;
        }
        let Some(ps) = self.part().sketches.get(d.sketch) else {
            return false;
        };
        let sk = &ps.sketch;
        let Some((e, _)) =
            super::overlay::pick_entity(&self.camera, rect, sk, pos, super::overlay::CURVE_SNAP_PX)
        else {
            return false;
        };
        if !matches!(
            sk.entity(e),
            Some(crate::sketch::Entity::Line { .. } | crate::sketch::Entity::Arc { .. })
        ) || ps.is_reference(e)
        {
            return false;
        }
        let bounds = self.sketch_regions(sk).iter().any(|r| {
            std::iter::once(&r.outer)
                .chain(&r.holes)
                .any(|l| l.edges.iter().any(|x| x.source == e))
        });
        if bounds {
            return false;
        }
        let Some(d) = self.extrude.as_mut() else {
            return false;
        };
        let DraftDef::Extrude(x) = &mut d.def else {
            return false;
        };
        let open = x.open.get_or_insert_with(|| crate::part::OpenProfile {
            curves: Vec::new(),
            flip: false,
            match_shape: true,
        });
        if let Some(k) = open.curves.iter().position(|&c| c == e) {
            open.curves.remove(k);
        } else {
            open.curves.push(e);
        }
        if open.curves.is_empty() {
            x.open = None;
        }
        true
    }

    /// The sketch whose dimensions can be edited in the view: the edited
    /// one, else the profile sketch of an open Extrude / Revolve (Inventor:
    /// visible sketch dimensions are editable during the preview).
    pub(super) fn dim_sketch(&self) -> Option<usize> {
        self.editing
            .or_else(|| self.extrude.as_ref().map(|d| d.sketch))
            .or_else(|| self.show_dims.as_ref().and_then(|s| s.sketch))
    }

    // ---- primitives --------------------------------------------------------

    /// 3D Model → Primitives → Box / Cylinder / Sphere / Torus: pick the
    /// sketch plane first.
    pub(super) fn start_primitive(&mut self, kind: PrimitiveKind) {
        self.finish_sketch();
        self.cancel_extrude();
        self.mode = Mode::PickPlane(PickPurpose::Primitive(kind));
        self.file_status = None;
    }

    /// The plane of a primitive was picked: an empty sketch on it, then the
    /// clicks.
    pub(super) fn primitive_plane(&mut self, kind: PrimitiveKind, plane: PlaneRef) {
        self.checkpoint(kind.label());
        let opts = SketchOptions {
            auto_project_origin: false,
            auto_reference_edges: false,
            ..self.sketch_opts.clone()
        };
        let placement = SketchPlacement {
            plane,
            ..Default::default()
        };
        match self.part_mut().create_sketch(placement, &opts) {
            Ok(i) => {
                self.prim_draft = Some(PrimDraft {
                    kind,
                    sketch: i,
                    clicks: Vec::new(),
                });
            }
            Err(e) => {
                self.history.cancel();
                self.file_status = Some(Err(format!("{}: {e}", kind.label())));
            }
        }
    }

    /// The point of the primitive's sketch under `pos` (snapped to the
    /// sketch origin within the point snap radius).
    fn primitive_point(&self, pos: egui::Pos2, rect: egui::Rect) -> Option<Point2> {
        let d = self.prim_draft.as_ref()?;
        let sk = &self.part().sketches.get(d.sketch)?.sketch;
        let w = self.camera.ray_plane(pos, rect, &sk.plane)?;
        let q = sk.plane.project(&w);
        let origin = self.camera.project(&sk.plane.point_at(0.0, 0.0), rect);
        if origin.is_some_and(|o| o.distance(pos) <= super::overlay::POINT_SNAP_PX) {
            return Some(Point2::origin());
        }
        Some(q)
    }

    /// A click while placing a primitive.
    pub(super) fn primitive_click(&mut self, pos: egui::Pos2, rect: egui::Rect) {
        let Some(q) = self.primitive_point(pos, rect) else {
            return;
        };
        let Some(d) = self.prim_draft.as_mut() else {
            return;
        };
        d.clicks.push(q);
        if d.clicks.len() < d.kind.clicks() {
            return;
        }
        let Some(d) = self.prim_draft.take() else {
            return;
        };
        let (kind, i) = (d.kind, d.sketch);
        let drawn =
            crate::part::primitive::draw(&mut self.part_mut().sketches[i].sketch, kind, &d.clicks);
        let axis = match drawn {
            Ok(a) => a,
            Err(e) => {
                // Try the last click again.
                self.file_status = Some(Err(format!("{}: {e}", kind.label())));
                let mut d = d;
                d.clicks.pop();
                self.prim_draft = Some(d);
                return;
            }
        };
        let _ = self.part_mut().sketches[i].sketch.solve();
        let regions = self.sketch_regions(&self.part().sketches[i].sketch);
        let profiles = crate::part::extrude::default_profiles(&self.part().sketches[i], &regions);
        let operation = crate::part::Operation::default_for(&self.part().body);
        let def = match axis {
            Some(line) => {
                let mut r = crate::part::RevolveFeature::new(
                    profiles,
                    crate::part::AxisRef::SketchLine(line),
                );
                r.operation = operation;
                DraftDef::Revolve(r)
            }
            None => {
                let mut e = crate::part::ExtrudeFeature::new(profiles, 10.0);
                e.extend_face = false;
                e.operation = operation;
                DraftDef::Extrude(e)
            }
        };
        self.part_mut().sketches[i].visible = false;
        let mut draft = ExtrudeDraft::new(i, def);
        draft.axis_set = axis.is_some();
        draft.temp_sketch = true;
        draft.primitive = Some(kind);
        self.extrude = Some(draft);
        self.file_status = None;
        self.scene_dirty = true;
    }

    /// Esc while placing a primitive: its sketch goes.
    pub(super) fn cancel_primitive(&mut self) {
        if let Some(d) = self.prim_draft.take() {
            let _ = self.part_mut().delete_sketch(d.sketch);
            self.history.cancel();
            self.scene_dirty = true;
        }
    }

    /// The rubber band of a primitive being placed.
    pub(super) fn draw_primitive_preview(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        pointer: Option<egui::Pos2>,
    ) {
        let Some(d) = self.prim_draft.as_ref() else {
            return;
        };
        let Some(sk) = self.part().sketches.get(d.sketch).map(|s| &s.sketch) else {
            return;
        };
        let mut pts = d.clicks.clone();
        if let Some(q) = pointer.and_then(|p| self.primitive_point(p, rect)) {
            pts.push(q);
        }
        let screen = |q: Point2| self.camera.project(&sk.plane.point_at(q.x, q.y), rect);
        let stroke = egui::Stroke::new(1.5, super::overlay::HIGHLIGHT);
        let ring = |c: Point2, r: f64| -> Vec<egui::Pos2> {
            (0..=64)
                .filter_map(|k| {
                    let t = std::f64::consts::TAU * k as f64 / 64.0;
                    screen(Point2::new(c.x + r * t.cos(), c.y + r * t.sin()))
                })
                .collect()
        };
        match (d.kind, pts.as_slice()) {
            (PrimitiveKind::Box, [c, q, ..]) => {
                let v = q - c;
                let corners = [
                    c - v,
                    Point2::new(c.x + v.x, c.y - v.y),
                    c + v,
                    Point2::new(c.x - v.x, c.y + v.y),
                    c - v,
                ];
                let s: Vec<_> = corners.into_iter().filter_map(screen).collect();
                painter.add(egui::Shape::line(s, stroke));
            }
            (PrimitiveKind::Cylinder | PrimitiveKind::Sphere, [c, q, ..]) => {
                painter.add(egui::Shape::line(ring(*c, (q - c).norm()), stroke));
            }
            (PrimitiveKind::Torus, [c, s, rest @ ..]) => {
                painter.add(egui::Shape::line(ring(*c, (s - c).norm()), stroke));
                if let Some(q) = rest.first() {
                    painter.add(egui::Shape::line(ring(*s, (q - s).norm()), stroke));
                }
            }
            _ => {}
        }
        for q in &d.clicks {
            if let Some(s) = screen(*q) {
                painter.circle_filled(s, 4.0, super::overlay::HIGHLIGHT);
            }
        }
    }

    /// The status-bar prompt of a primitive being placed or of the Extrude
    /// / Revolve command.
    pub(super) fn feature_prompt(&self) -> Option<String> {
        if let Some(d) = &self.prim_draft {
            return Some(format!("{} (Esc cancels)", d.kind.prompt(d.clicks.len())));
        }
        let d = self.extrude.as_ref()?;
        let cmd = d.def.command();
        Some(match &d.def {
            DraftDef::Revolve(_) if d.axis_pick => format!(
                "{cmd}: click a line of the sketch (or an origin axis in the panel) for the axis"
            ),
            _ if d.def.profiles().is_empty()
                && !matches!(&d.def, DraftDef::Extrude(e) if e.open.is_some()) =>
            {
                format!("{cmd}: click the profiles (regions) to use; open curves: Match Shape")
            }
            DraftDef::Revolve(_) => format!(
                "{cmd}: set the options, then OK (Ctrl+Enter); right-click: Continue to the axis"
            ),
            DraftDef::Extrude(_) => format!(
                "{cmd}: set the options (drag the arrow; double-click a dimension to edit it), then OK (Ctrl+Enter)"
            ),
        })
    }
}
