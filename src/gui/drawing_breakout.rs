//! Break Out (spec 4.11): the command (the view, a view sketch for the
//! profile, the dialog over the sheet with the depth and the From Point
//! pick in any view of the model) and Delete Break Out.

use eframe::egui::{self, Rect};

use super::CadCraftApp;
use super::drawing::{DrwTool, SheetCam, ShownView};
use crate::drawing::{BreakOut, BreakOutDepth};
use crate::sketch::Sketch;

/// Break Out › Depth types of the dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DepthKind {
    FromPoint,
    ToHole,
    ThroughPart,
}

/// The Break Out command after its profile: the view, the profile, the
/// depth being set.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct BoDraft {
    /// The view broken out.
    pub view: u32,
    /// The profile's loops (the view plane).
    pub profile: Vec<Vec<[f64; 2]>>,
    pub kind: DepthKind,
    /// From Point: the depth value (mm).
    pub value: f64,
    /// From Point: the picked point (model mm) and the view it was picked
    /// in (`None` for an edited break out's point).
    pub point: Option<([f64; 3], Option<u32>)>,
    /// To Hole: the hole's key, the point of its axis the cut goes through
    /// (model mm) and the view it was picked in (`None` when edited).
    pub hole: Option<(String, [f64; 3], Option<u32>)>,
    /// The sketch the profile was drawn in.
    pub sketch: Option<Box<crate::drawing::view_sketch::ViewSketch>>,
    /// Edit Break Out: the break out's index in the view.
    pub edit: Option<usize>,
}

impl BoDraft {
    /// Waiting for the depth of `profile` on `view`.
    pub fn new(view: u32, profile: Vec<Vec<[f64; 2]>>) -> Self {
        Self {
            view,
            profile,
            kind: DepthKind::FromPoint,
            value: 5.0,
            point: None,
            hole: None,
            sketch: None,
            edit: None,
        }
    }

    /// Edit Break Out of break out `k` of `view`.
    pub fn editing(view: u32, k: usize, b: &BreakOut) -> Self {
        let (kind, value, point, hole) = match &b.depth {
            BreakOutDepth::FromPoint {
                point,
                hole: Some(h),
                ..
            } => (
                DepthKind::ToHole,
                5.0,
                None,
                Some((h.clone(), *point, None)),
            ),
            BreakOutDepth::FromPoint { point, depth, .. } => {
                (DepthKind::FromPoint, *depth, Some((*point, None)), None)
            }
            BreakOutDepth::ThroughPart => (DepthKind::ThroughPart, 5.0, None, None),
        };
        Self {
            view,
            profile: b.profile.clone(),
            kind,
            value,
            point,
            hole,
            sketch: b.sketch.clone(),
            edit: Some(k),
        }
    }

    /// The break out, when its inputs are complete.
    pub fn break_out(&self) -> Option<BreakOut> {
        let depth = match self.kind {
            DepthKind::FromPoint => BreakOutDepth::FromPoint {
                point: self.point?.0,
                depth: self.value,
                hole: None,
            },
            DepthKind::ToHole => {
                let (key, point, _) = self.hole.clone()?;
                BreakOutDepth::FromPoint {
                    point,
                    depth: 0.0,
                    hole: Some(key),
                }
            }
            DepthKind::ThroughPart => BreakOutDepth::ThroughPart,
        };
        Some(BreakOut {
            profile: self.profile.clone(),
            depth,
            section_all: false,
            sketch: self.sketch.clone(),
            suppressed: false,
        })
    }

    /// The status-bar prompt.
    pub fn prompt(&self) -> &'static str {
        match (self.kind, self.point, &self.hole) {
            (DepthKind::FromPoint, None, _) => {
                "Break Out: click the point the depth starts from, in another view of the model"
            }
            (DepthKind::ToHole, _, None) => {
                "Break Out: click a hole of the model (in any view); the cut goes through its axis"
            }
            _ => "Break Out: set the depth, then OK (Esc: cancel)",
        }
    }
}

/// The profile of a break out sketch: the outermost closed loops of its
/// regions (sampled), in the view plane.
pub(super) fn profile_loops(sk: &Sketch) -> Vec<Vec<[f64; 2]>> {
    let outers: Vec<Vec<[f64; 2]>> = sk
        .regions()
        .iter()
        .map(|r| {
            r.outer
                .polygon(0.02)
                .iter()
                .map(|p| [p.x, p.y])
                .collect::<Vec<_>>()
        })
        .filter(|l| l.len() >= 3)
        .collect();
    let closed = |l: &Vec<[f64; 2]>| {
        let mut c = l.clone();
        if let Some(f) = c.first().copied() {
            c.push(f);
        }
        c
    };
    // A loop inside another is part of it (Break Out removes their union).
    outers
        .iter()
        .enumerate()
        .filter(|(k, l)| {
            !outers
                .iter()
                .enumerate()
                .any(|(j, o)| j != *k && crate::drawing::hlr::inside_loop(&closed(o), l[0]))
        })
        .map(|(_, l)| closed(l))
        .collect()
}

impl CadCraftApp {
    /// The running Break Out command (after its profile).
    pub(super) fn break_out_tool(
        &mut self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        rect: Rect,
        response: &egui::Response,
        views: &[ShownView],
    ) {
        let Some(DrwTool::BreakOut(mut d)) = self.drw.tool.clone() else {
            return;
        };
        // The profile on its view, and the picked point's view framed.
        let cam = self.drw.cam;
        if let Some(pl) = views
            .iter()
            .find(|v| v.id == d.view)
            .and_then(super::drawing_break::placement)
        {
            for l in &d.profile {
                let pts: Vec<egui::Pos2> = l
                    .iter()
                    .map(|q| cam.to_screen(rect, pl.to_sheet(*q)))
                    .collect();
                painter.add(egui::Shape::line(
                    pts,
                    egui::Stroke::new(2.0, egui::Color32::from_rgb(230, 120, 20)),
                ));
            }
        }
        let picked = match d.kind {
            DepthKind::FromPoint => d.point.and_then(|p| p.1),
            DepthKind::ToHole => d.hole.as_ref().and_then(|h| h.2),
            DepthKind::ThroughPart => None,
        };
        // To Hole: the hole under the pointer lit, the picked one marked.
        if d.kind == DepthKind::ToHole {
            let hover = response
                .hover_pos()
                .and_then(|p| hole_under(views, rect, cam, p));
            let lit = |key: &str, color: egui::Color32| {
                for v in views {
                    for piece in hole_pieces(v, rect, cam, key) {
                        painter.add(egui::Shape::line(piece, egui::Stroke::new(2.5, color)));
                    }
                }
            };
            if let Some((key, _, _)) = &d.hole {
                lit(key, egui::Color32::from_rgb(20, 90, 200));
            }
            if let Some((_, key)) = hover {
                lit(&key, egui::Color32::from_rgb(230, 160, 20));
            }
        }
        if let Some(pv) = picked
            && let Some(v) = views.iter().find(|v| v.id == pv)
        {
            painter.rect_stroke(
                Rect::from_two_pos(cam.to_screen(rect, v.lo), cam.to_screen(rect, v.hi)),
                0.0,
                egui::Stroke::new(1.5, egui::Color32::from_rgb(20, 90, 200)),
                egui::StrokeKind::Middle,
            );
        }
        let mut ok = false;
        let mut cancel = false;
        egui::Area::new(egui::Id::new("drw_breakout_options"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::RIGHT_TOP)
            .fixed_pos(rect.right_top() + egui::vec2(-10.0, 10.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(if d.edit.is_some() {
                            "Edit Break Out"
                        } else {
                            "Break Out"
                        })
                        .strong(),
                    );
                    ui.label(format!(
                        "Boundary Profile: ✔ {} loop{}",
                        d.profile.len(),
                        if d.profile.len() == 1 { "" } else { "s" }
                    ));
                    ui.label(egui::RichText::new("Depth").strong());
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut d.kind, DepthKind::FromPoint, "From Point")
                            .on_hover_text("A point in any view of the model and a depth behind it");
                        ui.add_enabled(false, egui::RadioButton::new(false, "To Sketch"))
                            .on_disabled_hover_text(
                                "Planned: sketched geometry of another view (docs/spec/4.11-break-out.md)",
                            );
                        ui.radio_value(&mut d.kind, DepthKind::ToHole, "To Hole")
                            .on_hover_text(
                                "The cut goes through the axis of a hole, picked in any view of the model",
                            );
                        ui.radio_value(&mut d.kind, DepthKind::ThroughPart, "Through Part")
                            .on_hover_text("Cut all through the part");
                    });
                    ui.add_enabled_ui(d.kind == DepthKind::FromPoint, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Point:");
                            ui.label(if d.point.is_some() {
                                "✔ picked (click again to change)"
                            } else {
                                "click a point in another view"
                            });
                        });
                        ui.horizontal(|ui| {
                            ui.label("Depth");
                            ui.add(super::valuefield::ExprValue::new(&mut d.value).suffix(" mm"))
                                .on_hover_text("How far behind the point the cut lies");
                        });
                    });
                    if d.kind == DepthKind::ToHole {
                        ui.horizontal(|ui| {
                            ui.label("Hole:");
                            ui.label(match &d.hole {
                                Some((k, _, _)) => format!("✔ {k} (click again to change)"),
                                None => "click a hole in a view".into(),
                            });
                        });
                    }
                    ui.label(egui::RichText::new("Display").strong());
                    ui.add_enabled(false, egui::Checkbox::new(&mut false, "Show Hidden Edges"))
                        .on_disabled_hover_text(
                            "Planned: hidden edges shown for the pick (docs/spec/4.11-break-out.md)",
                        );
                    ui.add_enabled(false, egui::Checkbox::new(&mut false, "Section All Parts"))
                        .on_disabled_hover_text(
                            "Planned: assemblies (docs/spec/4.11-break-out.md)",
                        );
                    ui.separator();
                    ui.horizontal(|ui| {
                        let ready = d.break_out().is_some();
                        ok = ui.add_enabled(ready, egui::Button::new("OK")).clicked();
                        cancel = ui.button("Cancel").clicked();
                    });
                });
            });
        // From Point: a point of another view of the same model.
        if d.kind == DepthKind::FromPoint
            && response.clicked()
            && let Some(p) = response.hover_pos()
        {
            match self.break_out_point(views, rect, p, d.view) {
                Ok((pt, v)) => d.point = Some((pt, Some(v))),
                Err(e) => self.file_status = Some(Err(format!("Break Out: {e}"))),
            }
        }
        // To Hole: a hole of the same model, its axis across the view.
        if d.kind == DepthKind::ToHole
            && response.clicked()
            && let Some(p) = response.hover_pos()
        {
            match self.break_out_hole(views, rect, p, d.view) {
                Ok((key, pt, v)) => d.hole = Some((key, pt, Some(v))),
                Err(e) => self.file_status = Some(Err(format!("Break Out: {e}"))),
            }
        }
        if cancel {
            self.drw.tool = None;
            return;
        }
        if ok && let Some(b) = d.break_out() {
            let view = d.view;
            let r = match d.edit {
                Some(k) => self.edit_drawing("Edit Break Out", move |doc| {
                    doc.set_break_out(view, k, b).then_some(k)
                }),
                None => self.edit_drawing("Break Out", move |doc| doc.add_break_out(view, b)),
            };
            match r {
                Some(Some(_)) => {
                    self.drw.tool = None;
                    self.drw.sel = Some(view);
                    self.file_status = Some(Ok("Break Out: done".into()));
                }
                _ => {
                    self.history.cancel();
                    self.file_status =
                        Some(Err("Break Out: the profile has no closed loop".into()));
                }
            }
            return;
        }
        self.drw.tool = Some(DrwTool::BreakOut(d));
    }

    /// The model point under `p` in a view other than `view` showing the
    /// same model (an end point, a centre or a point on an edge): its view
    /// plane position lifted into the model (the depth along that view is
    /// unknown, so the point counts only across it).
    fn break_out_point(
        &self,
        views: &[ShownView],
        rect: Rect,
        p: egui::Pos2,
        view: u32,
    ) -> Result<([f64; 3], u32), String> {
        let g = self
            .geometry_under(views, rect, p)
            .ok_or("click an edge or a point of another view of the model")?;
        if g.view == view {
            return Err("pick the point in another view of the model (it gives the depth)".into());
        }
        let doc = self.drawing().ok_or("no drawing")?;
        let sheet = doc.sheet().ok_or("no sheet")?;
        if doc.model_of(sheet, g.view) != doc.model_of(sheet, view) {
            return Err("that view shows another model".into());
        }
        let frame = doc
            .frame_of(sheet, g.view)
            .ok_or("that view has no direction")?;
        let pv = views
            .iter()
            .find(|v| v.id == g.view)
            .ok_or("that view is not shown")?;
        let pl = super::drawing_break::placement(pv).ok_or("that view is not computed yet")?;
        let q = pl.to_view(g.at);
        let m = frame.right * q[0] + frame.up * q[1];
        Ok(([m.x, m.y, m.z], g.view))
    }

    /// The hole under `p` in a view of the same model as `view`: its key and
    /// the point of its axis the cut of `view` goes through.
    fn break_out_hole(
        &self,
        views: &[ShownView],
        rect: Rect,
        p: egui::Pos2,
        view: u32,
    ) -> Result<(String, [f64; 3], u32), String> {
        let (pv, key) = hole_under(views, rect, self.drw.cam, p)
            .ok_or("click a hole's edge (a circle, or its outline seen from the side)")?;
        let doc = self.drawing().ok_or("no drawing")?;
        let sheet = doc.sheet().ok_or("no sheet")?;
        let model = doc.model_of(sheet, view).ok_or("the view has no model")?;
        if doc.model_of(sheet, pv) != Some(model) {
            return Err("that hole is in another model".into());
        }
        let frame = doc
            .frame_of(sheet, view)
            .ok_or("the view has no direction")?;
        let h = self
            .with_part(model, |part| {
                part.holes().iter().find(|h| h.key() == key).cloned()
            })
            .flatten()
            .ok_or("holes of assembly views are not supported yet")?;
        let pt = crate::drawing::break_out_hole_point(&h, &frame)?;
        Ok((key, pt, pv))
    }

    /// Edit Break Out of break out `k` of view `view`: the dialog again.
    pub(super) fn open_break_out_edit(&mut self, view: u32, k: usize) {
        let b = self
            .drawing()
            .and_then(|d| d.sheet()?.view(view)?.break_outs.get(k).cloned());
        if let Some(b) = b {
            self.drw.sel = Some(view);
            self.drw.tool = Some(DrwTool::BreakOut(BoDraft::editing(view, k, &b)));
        }
    }

    /// Edit Break Out Sketch of break out `k` of view `view`: its sketch
    /// opens (one drawn from the profile when it has none); Finish Sketch
    /// sets the new profile.
    pub(super) fn open_break_out_sketch_edit(&mut self, view: u32, k: usize) {
        let b = self
            .drawing()
            .and_then(|d| d.sheet()?.view(view)?.break_outs.get(k).cloned());
        let Some(b) = b else {
            return;
        };
        let sketch = b.sketch.as_deref().cloned().unwrap_or_else(|| {
            let mut s = crate::drawing::view_sketch::ViewSketch::default();
            for l in &b.profile {
                let mut pts = l.clone();
                if pts.len() > 1 && pts.first() == pts.last() {
                    pts.pop();
                }
                let ids: Vec<_> = pts
                    .iter()
                    .map(|p| s.0.add_point(crate::sketch::Point2::new(p[0], p[1])))
                    .collect();
                for (i, a) in ids.iter().enumerate() {
                    let _ = s.0.add_line_between(*a, ids[(i + 1) % ids.len()]);
                }
            }
            s
        });
        self.open_view_sketch(
            view,
            super::drawing_sketch::SketchFor::BreakOutEdit(k),
            sketch,
        );
    }

    /// Delete Break Out: break out `k` of view `view` (one Undo step).
    pub(super) fn delete_break_out(&mut self, view: u32, k: usize) {
        if self.edit_drawing("Delete Break Out", move |d| d.remove_break_out(view, k)) != Some(true)
        {
            self.history.cancel();
        }
        self.history.break_coalescing();
    }
}

/// The hole whose edges (visible or hidden) pass nearest the pointer, within
/// a few pixels: its view and key.
fn hole_under(
    views: &[ShownView],
    rect: Rect,
    cam: SheetCam,
    p: egui::Pos2,
) -> Option<(u32, String)> {
    let mut best: Option<(f32, u32, String)> = None;
    for v in views {
        let Some(l) = v.lines.as_deref() else {
            continue;
        };
        let mut seen = std::collections::HashSet::new();
        for key in l.edges.iter().filter_map(|e| e.hole.as_deref()) {
            if !seen.insert(key) {
                continue;
            }
            let d = hole_pieces(v, rect, cam, key)
                .iter()
                .flat_map(|piece| piece.windows(2).map(|w| seg_distance(p, w[0], w[1])))
                .fold(f32::MAX, f32::min);
            if d <= 7.0 && best.as_ref().is_none_or(|b| d < b.0) {
                best = Some((d, v.id, key.to_string()));
            }
        }
    }
    best.map(|(_, v, k)| (v, k))
}

/// The screen pieces (visible and hidden) of hole `key`'s edges in view `v`.
fn hole_pieces(v: &ShownView, rect: Rect, cam: SheetCam, key: &str) -> Vec<Vec<egui::Pos2>> {
    let (Some(l), Some(pl)) = (v.lines.as_deref(), super::drawing_break::placement(v)) else {
        return Vec::new();
    };
    let of_hole = |src: &Option<u32>| {
        src.and_then(|k| l.edges.get(k as usize))
            .is_some_and(|e| e.hole.as_deref() == Some(key))
    };
    let vis = l.visible.iter().zip(l.visible_src.iter());
    let hid = l.hidden.iter().zip(l.hidden_src.iter());
    vis.chain(hid)
        .filter(|(_, src)| of_hole(src))
        .map(|(piece, _)| {
            piece
                .iter()
                .map(|q| cam.to_screen(rect, pl.to_sheet(*q)))
                .collect()
        })
        .collect()
}

/// Screen distance from `p` to the segment `a`-`b`.
fn seg_distance(p: egui::Pos2, a: egui::Pos2, b: egui::Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    (a + ab * t).distance(p)
}
