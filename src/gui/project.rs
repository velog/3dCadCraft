//! Sketch → Create → **Project Geometry** (`docs/spec/1.8-project-geometry.md`).
//!
//! While the command is on (ribbon toggle; Esc or right-click → Done ends
//! it), a click in the view projects into the edited sketch, in this order
//! of priority:
//! 1. a point of an earlier sketch (centre points, line ends, spline
//!    points), within [`overlay::POINT_SNAP_PX`];
//! 2. a curve of an earlier sketch (line, circle, arc, spline), within
//!    [`overlay::CURVE_SNAP_PX`];
//! 3. a model edge: straight edges by their vertices
//!    (`PartDocument::project_edge`), circular ones as linked circles/arcs
//!    (`PartDocument::project_curved_edge`).
//!
//! Earlier sketches are drawn while the command runs, also hidden (consumed)
//! ones, so their geometry can be picked. Shift+click projects a sketch curve
//! as construction geometry (as construction-off, with Construction on).
//! **Replace Reference** (References row or the context menu) arms
//! [`CadCraftApp::relink`]: the next pick replaces that link's source.

use eframe::egui::{self, Color32, Painter, Pos2, Rect, Stroke};

use super::{CadCraftApp, overlay};
use crate::kernel::EdgeId;
use crate::part::{LinkSource, SketchGeo};

/// What a Project Geometry click would take.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum ProjPick {
    /// Geometry of sketch `sketch` (index).
    Sketch {
        /// Source sketch index.
        sketch: usize,
        /// The point or curve.
        geo: SketchGeo,
    },
    /// A model edge.
    Edge(EdgeId),
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

    /// The geometry a Project Geometry click at `p` would take.
    pub(super) fn project_pick(&self, i: usize, p: Pos2, rect: Rect) -> Option<ProjPick> {
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
                if d <= overlay::POINT_SNAP_PX && best.is_none_or(|(bd, _)| d < bd) {
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
                if best.is_none_or(|(bd, _)| d < bd) {
                    let geo = SketchGeo::Curve { entity: e };
                    best = Some((d, ProjPick::Sketch { sketch: j, geo }));
                }
            }
        }
        if let Some((_, pick)) = best {
            return Some(pick);
        }
        self.edge_under(p, rect).map(ProjPick::Edge)
    }

    /// Project Geometry: a click at `p` projects what is under it into
    /// sketch `i` (one Undo step), or replaces the source of the armed
    /// Replace Reference. Returns whether the click was taken.
    pub(super) fn project_click(&mut self, i: usize, p: Pos2, rect: Rect, shift: bool) -> bool {
        let Some(pick) = self.project_pick(i, p, rect) else {
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
                };
                let Some(source) = source else {
                    self.file_status = Some(Err(
                        "Replace Reference: that edge has no persistent name".into(),
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
            ProjPick::Edge(e) if self.part().body.edge_direction(e).is_some() => {
                self.part_mut().project_edge(i, e).map(|_| ())
            }
            ProjPick::Edge(e) => self.part_mut().project_curved_edge(i, e).map(|_| ()),
        };
        match r {
            Ok(()) => {
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

    /// Lights up what a Project Geometry click at `pointer` would take.
    pub(super) fn draw_project_hover(&self, painter: &Painter, rect: Rect, pointer: Option<Pos2>) {
        let (Some(i), Some(p)) = (self.editing, pointer) else {
            return;
        };
        if !self.project_geometry {
            return;
        }
        let col = overlay::HIGHLIGHT;
        match self.project_pick(i, p, rect) {
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
