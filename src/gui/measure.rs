//! The Measure window (Inspect → Measure, `M`): measures the selection
//! (one or two faces, edges, vertices or bodies) with `part::measure` and
//! draws the shortest distance in the view. While it is open a plain click
//! adds to the selection (the third click starts a new pair), as Inventor's
//! Measure takes the first and the second item one after the other.

use eframe::egui::{self, Color32, RichText, Stroke};

use super::CadCraftApp;
use super::select::SelItem;
use crate::part::measure::{self, Measurement, Target};

/// State of the Measure window.
#[derive(Default)]
pub(super) struct MeasureUi {
    pub open: bool,
    /// The last result and what it was computed from (body + selection):
    /// the closest pair is too heavy for every frame.
    cache: std::cell::RefCell<Option<(String, Result<Measurement, String>)>>,
}

/// The measurable targets of the selection (the last two), and whether
/// something in it was skipped (features, sketches).
pub(super) fn targets(sel: &[SelItem]) -> (Vec<Target>, bool) {
    let mut out = Vec::new();
    let mut skipped = false;
    for s in sel {
        match s {
            SelItem::Face(n) => out.push(Target::Face(n.clone())),
            SelItem::Edge(e) => out.push(Target::Edge(*e)),
            SelItem::Vertex(n) => out.push(Target::Vertex(n.clone())),
            SelItem::Body(n) => out.push(Target::Body(n.clone())),
            SelItem::Feature(_) | SelItem::Sketch(_) => skipped = true,
        }
    }
    let n = out.len();
    (out.split_off(n.saturating_sub(2)), skipped)
}

fn mm(v: f64) -> String {
    format!("{v:.3} mm")
}

impl CadCraftApp {
    /// Inspect → Measure: opens (or closes) the window.
    pub(super) fn toggle_measure(&mut self) {
        self.measure.open = !self.measure.open;
    }

    /// The measurement of the current selection (cached per body and
    /// selection).
    fn measurement(&self) -> Option<Result<Measurement, String>> {
        let (t, _) = targets(&self.sel);
        if t.is_empty() {
            return None;
        }
        let key = format!("{}|{:?}", self.body_key(), t);
        if let Some((k, r)) = &*self.measure.cache.borrow()
            && *k == key
        {
            return Some(r.clone());
        }
        let r = measure::measure(&self.part().body, &t);
        *self.measure.cache.borrow_mut() = Some((key, r.clone()));
        Some(r)
    }

    /// While Measure is open a plain click adds the item (two at most:
    /// a third starts a new pair).
    pub(super) fn measure_click(&mut self, add: bool, toggle: bool) -> bool {
        if !self.measure.open || add || toggle {
            return add;
        }
        if self.sel.len() >= 2 {
            self.sel.clear();
        }
        true
    }

    pub(super) fn measure_window(&mut self, ctx: &egui::Context) {
        if !self.measure.open {
            return;
        }
        let mut open = true;
        let result = self.measurement();
        let (_, skipped) = targets(&self.sel);
        self.layout
            .place(
                "measure",
                ctx.content_rect(),
                true,
                egui::Window::new("Measure")
                    .open(&mut open)
                    .default_width(320.0)
                    .resizable(true),
            )
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(
                        "Click a face, edge, vertex or body; click a second one for the \
                         distance and angle (a third click starts again).",
                    )
                    .weak(),
                );
                ui.separator();
                match &result {
                    None => {
                        ui.label("Nothing selected.");
                    }
                    Some(Err(e)) => {
                        ui.colored_label(Color32::from_rgb(200, 120, 0), format!("Measure: {e}"));
                    }
                    Some(Ok(m)) => {
                        egui::Grid::new("measure_grid")
                            .num_columns(2)
                            .show(ui, |ui| {
                                for (k, it) in m.items.iter().enumerate() {
                                    ui.label(
                                        RichText::new(format!("{} {}", it.kind, k + 1)).strong(),
                                    );
                                    ui.end_row();
                                    let mut row = |name: &str, v: String| {
                                        ui.label(name);
                                        ui.label(v);
                                        ui.end_row();
                                    };
                                    if let Some(l) = it.length {
                                        row("Length", mm(l));
                                    }
                                    if let Some(r) = it.radius {
                                        row("Radius", mm(r));
                                        row("Diameter", mm(2.0 * r));
                                    }
                                    if let Some(a) = it.arc {
                                        row("Arc angle", format!("{:.3}°", a.to_degrees()));
                                    }
                                    if let Some(a) = it.area {
                                        row("Area", format!("{a:.3} mm²"));
                                    }
                                    if let Some(v) = it.volume {
                                        row("Volume", format!("{v:.3} mm³"));
                                    }
                                    if let Some(p) = it.position {
                                        let what = if it.kind == "Vertex" || it.kind == "Point" {
                                            "Position"
                                        } else {
                                            "Centre"
                                        };
                                        row(what, format!("({:.3}, {:.3}, {:.3})", p.x, p.y, p.z));
                                    }
                                }
                                if m.items.len() == 2 {
                                    ui.label(RichText::new("Between").strong());
                                    ui.end_row();
                                    if let Some((d, p, q)) = m.distance {
                                        ui.label("Minimum distance");
                                        ui.label(mm(d));
                                        ui.end_row();
                                        let v = q - p;
                                        ui.label("dX, dY, dZ");
                                        ui.label(format!("{:.3}, {:.3}, {:.3}", v.x, v.y, v.z));
                                        ui.end_row();
                                    }
                                    if let Some(c) = m.center_distance {
                                        ui.label("Centre distance");
                                        ui.label(mm(c));
                                        ui.end_row();
                                    }
                                    if let Some(a) = m.angle_deg {
                                        ui.label("Angle");
                                        ui.label(format!("{a:.3}°"));
                                        ui.end_row();
                                    }
                                }
                            });
                    }
                }
                if skipped {
                    ui.label(
                        RichText::new("Features and sketches in the selection are not measured.")
                            .weak(),
                    );
                }
                ui.separator();
                if ui.button("Clear Selection").clicked() {
                    self.sel.clear();
                }
            });
        if !open {
            self.measure.open = false;
        }
    }

    /// The shortest distance of a measured pair: a line between the two
    /// closest points with its value.
    pub(super) fn draw_measure(&self, painter: &egui::Painter, rect: egui::Rect) {
        if !self.measure.open {
            return;
        }
        let Some(Ok(m)) = self.measurement() else {
            return;
        };
        let Some((d, p, q)) = m.distance else { return };
        let (Some(a), Some(b)) = (self.camera.project(&p, rect), self.camera.project(&q, rect))
        else {
            return;
        };
        let col = Color32::from_rgb(230, 120, 20);
        painter.line_segment([a, b], Stroke::new(2.0, col));
        painter.circle_filled(a, 3.5, col);
        painter.circle_filled(b, 3.5, col);
        let mid = a + (b - a) * 0.5 + egui::vec2(6.0, -6.0);
        painter.text(
            mid,
            egui::Align2::LEFT_BOTTOM,
            mm(d),
            egui::FontId::proportional(14.0),
            col,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_keep_the_last_two_measurable_items() {
        let sel = vec![
            SelItem::Face("A".into()),
            SelItem::Feature("Extrusion1".into()),
            SelItem::Face("B".into()),
            SelItem::Vertex("V".into()),
        ];
        let (t, skipped) = targets(&sel);
        assert!(skipped);
        assert_eq!(
            t,
            vec![Target::Face("B".into()), Target::Vertex("V".into())]
        );
        assert!(targets(&[]).0.is_empty());
    }
}
