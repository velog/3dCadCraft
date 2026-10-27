//! The Measure window (Inspect → Measure, `M`): measures the selection
//! (one or two faces, edges, vertices or bodies) with `part::measure` and
//! draws the shortest distance in the view. While it is open a plain click
//! adds to the selection (the third click starts a new pair), as Inventor's
//! Measure takes the first and the second item one after the other.

use eframe::egui::{self, Color32, RichText, Stroke};

use super::CadCraftApp;
use super::select::SelItem;
use crate::assembly::relation::GeomRef;
use crate::kernel::{BRep, EdgeId, Transform3};
use crate::part::blend::EdgeRef;
use crate::part::measure::{self, Measurement, Target};

/// State of the Measure window.
#[derive(Default)]
pub(super) struct MeasureUi {
    pub open: bool,
    /// The last result and what it was computed from (body + selection):
    /// the closest pair is too heavy for every frame.
    cache: std::cell::RefCell<Option<(String, Result<Measurement, String>)>>,
    /// Measure Loop: a picked edge gives the length of its loop.
    pub loop_mode: bool,
    /// Accumulate: values added with "Add to Accumulate" (label, mm).
    pub acc: Vec<(String, f64)>,
    /// In an assembly: the picked faces / edges of components (two at
    /// most; a third click starts a new pair).
    pub asm_picks: Vec<GeomRef>,
    /// In an assembly: the face / edge under the cursor.
    pub asm_hover: Option<GeomRef>,
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

    /// The right-click menu's Measure: opens the window on the selection.
    pub(super) fn measure_context(&mut self, ui: &mut egui::Ui) {
        if ui
            .button("Measure")
            .on_hover_text("Measure the selection (M)")
            .clicked()
        {
            self.measure.open = true;
            ui.close();
        }
    }

    /// Measure Loop of the selected edge: (length, edge count).
    fn measured_loop(&self) -> Option<(f64, usize)> {
        let e = self.sel.iter().rev().find_map(|s| match s {
            SelItem::Edge(e) => Some(*e),
            _ => None,
        })?;
        // A selected face chooses which of the edge's two loops.
        let face = self.sel.iter().find_map(|s| match s {
            SelItem::Face(n) => Some(n.as_str()),
            _ => None,
        });
        measure::loop_length(&self.part().body, e, face).map(|(l, es)| (l, es.len()))
    }

    /// The value "Add to Accumulate" takes: the loop, the distance of a
    /// pair, else a length or an area.
    fn accumulable(&self, m: Option<&Measurement>) -> Option<(String, f64)> {
        if self.measure.loop_mode {
            return self.measured_loop().map(|(l, _)| ("Loop".to_owned(), l));
        }
        let m = m?;
        if let Some((d, ..)) = m.distance {
            return Some(("Distance".to_owned(), d));
        }
        let it = m.items.last()?;
        it.length
            .map(|l| ("Length".to_owned(), l))
            .or(it.area.map(|a| ("Area".to_owned(), a)))
    }

    /// The measurement of the current selection (cached per body and
    /// selection); in an assembly, of the picked component faces / edges.
    fn measurement(&self) -> Option<Result<Measurement, String>> {
        if self.is_assembly() {
            return self.asm_measurement();
        }
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

    /// The measurement of the faces / edges picked in an assembly, each in
    /// its part and moved by its occurrence (cached by picks and places).
    fn asm_measurement(&self) -> Option<Result<Measurement, String>> {
        let a = self.assembly()?;
        let picks = &self.measure.asm_picks;
        if picks.is_empty() {
            return None;
        }
        let mut key = format!("asm|{picks:?}");
        for g in picks {
            let Some(occ) = a.find(g.occurrence()) else {
                return Some(Err("the component is gone".into()));
            };
            let sig = self.with_part(occ.component, |p| p.body_signature())?;
            key.push_str(&format!("|{sig}|{:?}", occ.transform));
        }
        if let Some((k, r)) = &*self.measure.cache.borrow()
            && *k == key
        {
            return Some(r.clone());
        }
        let mut placed: Vec<(BRep, Target, Transform3)> = Vec::new();
        for g in picks {
            let occ = a.find(g.occurrence())?;
            let item = self.with_part(occ.component, |p| {
                let t = match g {
                    GeomRef::Face { name, .. } => Target::Face(name.clone()),
                    GeomRef::Edge { edge, .. } => Target::Edge(edge_by_ref(&p.body, edge)?),
                    _ => return None,
                };
                Some((p.body.clone(), t))
            })?;
            let Some((body, t)) = item else {
                return Some(Err("this item can't be measured".into()));
            };
            placed.push((body, t, occ.transform));
        }
        let refs: Vec<measure::Placed> =
            placed.iter().map(|(b, t, m)| (b, t.clone(), *m)).collect();
        let r = measure::measure_placed(&refs);
        *self.measure.cache.borrow_mut() = Some((key, r.clone()));
        Some(r)
    }

    /// Measure in an assembly view (no command open): the face / edge
    /// under the cursor is highlighted, a click picks it (a third click
    /// starts a new pair, a click on nothing clears), the picks and the
    /// distance are drawn.
    pub(super) fn asm_measure_view(
        &mut self,
        painter: &egui::Painter,
        rect: egui::Rect,
        pointer: Option<egui::Pos2>,
        clicked: bool,
    ) {
        if !self.measure.open {
            self.measure.asm_picks.clear();
            self.measure.asm_hover = None;
            return;
        }
        self.measure.asm_hover = pointer.and_then(|p| self.geom_under_pub(p, rect));
        if clicked {
            match self.measure.asm_hover.clone() {
                Some(g) => {
                    if self.measure.asm_picks.len() >= 2 {
                        self.measure.asm_picks.clear();
                    }
                    self.measure.asm_picks.push(g);
                }
                None => self.measure.asm_picks.clear(),
            }
        }
        let picks = self.measure.asm_picks.clone();
        for (k, g) in picks.iter().enumerate() {
            let col = if k == 0 {
                Color32::from_rgb(80, 160, 255)
            } else {
                Color32::from_rgb(200, 110, 255)
            };
            self.draw_geom(painter, rect, g, col);
        }
        if let Some(g) = self.measure.asm_hover.clone() {
            self.draw_geom(painter, rect, &g, Color32::from_rgb(110, 220, 120));
        }
        self.draw_measure(painter, rect);
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
                let asm = self.is_assembly();
                if asm {
                    self.measure.loop_mode = false;
                } else {
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.measure.loop_mode, false, "Measure");
                        ui.selectable_value(&mut self.measure.loop_mode, true, "Loop")
                            .on_hover_text("Click an edge: the length of its closed loop");
                    });
                }
                ui.label(
                    RichText::new(if self.measure.loop_mode {
                        "Click an edge: the length of the loop it belongs to (with a face selected too: that face's loop)."
                    } else if asm {
                        "Click a face or an edge of a component; click a second one (on \
                         the same or another component) for the distance and angle (a \
                         third click starts again)."
                    } else {
                        "Click a face, edge, vertex or body; click a second one for the \
                         distance and angle (a third click starts again)."
                    })
                    .weak(),
                );
                ui.separator();
                if self.measure.loop_mode {
                    match self.measured_loop() {
                        Some((l, n)) => {
                            ui.label(RichText::new(format!("Loop length: {}", mm(l))).strong());
                            ui.label(RichText::new(format!("{n} edge pieces")).weak());
                        }
                        None => {
                            ui.label("Select an edge.");
                        }
                    }
                }
                match &result {
                    None => {
                        ui.label("Nothing selected.");
                    }
                    Some(Err(e)) => {
                        ui.colored_label(Color32::from_rgb(200, 120, 0), format!("Measure: {e}"));
                    }
                    Some(Ok(_)) if self.measure.loop_mode => {}
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
                // Accumulate (Inventor): a running sum of measured values.
                let value = self.accumulable(result.as_ref().and_then(|r| r.as_ref().ok()));
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(value.is_some(), egui::Button::new("Add to Accumulate"))
                        .clicked()
                        && let Some(v) = value
                    {
                        self.measure.acc.push(v);
                    }
                    if ui
                        .add_enabled(!self.measure.acc.is_empty(), egui::Button::new("Clear Accumulate"))
                        .clicked()
                    {
                        self.measure.acc.clear();
                    }
                });
                if !self.measure.acc.is_empty() {
                    let total: f64 = self.measure.acc.iter().map(|(_, v)| v).sum();
                    let parts: Vec<String> =
                        self.measure.acc.iter().map(|(k, v)| format!("{k} {v:.3}")).collect();
                    ui.label(RichText::new(format!("Accumulate: {total:.3}")).strong())
                        .on_hover_text(parts.join(" + "));
                }
                if ui.button("Clear Selection").clicked() {
                    self.sel.clear();
                    self.measure.asm_picks.clear();
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

/// The edge of `body` an [`EdgeRef`] names: between its two faces, the
/// piece nearest its hint.
fn edge_by_ref(body: &BRep, r: &EdgeRef) -> Option<EdgeId> {
    let hint = crate::kernel::Point3::from(r.hint);
    body.edges()
        .filter_map(|(e, _)| {
            let x = crate::part::blend::edge_ref(body, e)?;
            let same = x.faces == r.faces || (x.faces[0] == r.faces[1] && x.faces[1] == r.faces[0]);
            same.then(|| (e, (crate::kernel::Point3::from(x.hint) - hint).norm()))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(e, _)| e)
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
