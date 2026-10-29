//! Break Out (spec 4.11): the command (the view, a view sketch for the
//! profile, the dialog over the sheet with the depth and the From Point
//! pick in any view of the model) and Delete Break Out.

use eframe::egui::{self, Rect};

use super::CadCraftApp;
use super::drawing::{DrwTool, ShownView};
use crate::drawing::{BreakOut, BreakOutDepth};
use crate::sketch::Sketch;

/// Break Out › Depth types of the dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DepthKind {
    FromPoint,
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
    /// in.
    pub point: Option<([f64; 3], u32)>,
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
        }
    }

    /// The break out, when its inputs are complete.
    pub fn break_out(&self) -> Option<BreakOut> {
        let depth = match self.kind {
            DepthKind::FromPoint => BreakOutDepth::FromPoint {
                point: self.point?.0,
                depth: self.value,
            },
            DepthKind::ThroughPart => BreakOutDepth::ThroughPart,
        };
        Some(BreakOut {
            profile: self.profile.clone(),
            depth,
            section_all: false,
        })
    }

    /// The status-bar prompt.
    pub fn prompt(&self) -> &'static str {
        match (self.kind, self.point) {
            (DepthKind::FromPoint, None) => {
                "Break Out: click the point the depth starts from, in another view of the model"
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
        if let Some((_, pv)) = d.point
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
                    ui.label(egui::RichText::new("Break Out").strong());
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
                        ui.add_enabled(false, egui::RadioButton::new(false, "To Hole"))
                            .on_disabled_hover_text(
                                "Planned: the axis of a hole in the view (docs/spec/4.11-break-out.md)",
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
                Ok(pt) => d.point = Some(pt),
                Err(e) => self.file_status = Some(Err(format!("Break Out: {e}"))),
            }
        }
        if cancel {
            self.drw.tool = None;
            return;
        }
        if ok && let Some(b) = d.break_out() {
            let view = d.view;
            match self.edit_drawing("Break Out", move |doc| doc.add_break_out(view, b)) {
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

    /// Delete Break Out: the last break out of view `view` (one Undo step).
    pub(super) fn delete_break_out(&mut self, view: u32, k: usize) {
        if self.edit_drawing("Delete Break Out", move |d| d.remove_break_out(view, k)) != Some(true)
        {
            self.history.cancel();
        }
        self.history.break_coalescing();
    }
}
