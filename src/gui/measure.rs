//! The Measure window (Inspect → Measure, `M`): measures the selection
//! (one or two faces, edges, vertices or bodies) with `part::measure` and
//! draws the shortest distance in the view. While it is open a plain click
//! adds to the selection (the third click starts a new pair), as Inventor's
//! Measure takes the first and the second item one after the other.

use eframe::egui::{self, Color32, RichText, Stroke};

use super::CadCraftApp;
use super::select::SelItem;
use crate::assembly::relation::GeomRef;
use crate::kernel::Point3;
use crate::kernel::{BRep, EdgeId, Transform3};
use crate::part::blend::EdgeRef;
use crate::part::measure::{self, Measurement, SnapKind, Target};

/// The axes' colours in the view (the triad's): X, Y, Z.
const AXIS_COLORS: [Color32; 3] = [
    Color32::from_rgb(215, 50, 40),
    Color32::from_rgb(30, 150, 60),
    Color32::from_rgb(40, 100, 225),
];

/// A value in the view, centred on `at`, on a plate so that neither its line
/// nor the model runs through the digits.
fn value_plate(painter: &egui::Painter, at: egui::Pos2, text: String, c: Color32) {
    let galley = painter.layout_no_wrap(text, egui::FontId::proportional(13.0), c);
    let r = egui::Rect::from_center_size(at, galley.size() + egui::vec2(8.0, 4.0));
    painter.rect_filled(r, 3.0, Color32::from_rgba_unmultiplied(255, 255, 255, 225));
    painter.rect_stroke(r, 3.0, Stroke::new(1.0, c), egui::StrokeKind::Inside);
    painter.galley(r.min + egui::vec2(4.0, 2.0), galley, c);
}

/// An item picked for Measure in an assembly.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum AsmPick {
    /// A face or an edge of a component.
    Geom(GeomRef),
    /// A vertex, an edge's middle or a circle's centre of a part: its
    /// occurrence, the path to the part and where it is on the part.
    Point {
        occurrence: u32,
        sub: Vec<u32>,
        kind: SnapKind,
        local: [f64; 3],
    },
}

/// A body's snap points and box, built once while Measure is open.
type Snaps = std::rc::Rc<(Vec<(SnapKind, Point3)>, Point3, Point3)>;

fn snaps_of(body: &BRep) -> Snaps {
    let list = measure::snap_points(body);
    let mut lo = Point3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let mut hi = -lo;
    for (_, p) in &list {
        lo = lo.inf(p);
        hi = hi.sup(p);
    }
    // A flat or empty box still has to be hit by a ray passing nearby.
    let grow = ((hi - lo).norm() * 0.02).max(1.0);
    let g = crate::kernel::Vector3::new(grow, grow, grow);
    std::rc::Rc::new((list, lo - g, hi + g))
}

/// The snap radius: a vertex takes more room than what lies along an edge,
/// so that the edge itself stays easy to pick.
fn snap_radius(kind: SnapKind) -> f32 {
    match kind {
        SnapKind::Vertex => super::overlay::POINT_SNAP_PX,
        _ => 8.0,
    }
}

fn dot_at(painter: &egui::Painter, s: egui::Pos2, kind: SnapKind, col: Color32, label: bool) {
    painter.circle_filled(s, 5.0, col);
    painter.circle_stroke(s, 5.0, Stroke::new(1.0, Color32::WHITE));
    if label {
        painter.text(
            s + egui::vec2(8.0, 8.0),
            egui::Align2::LEFT_TOP,
            kind.label(),
            egui::FontId::proportional(12.0),
            col,
        );
    }
}

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
    /// In an assembly: the picked points, faces and edges of components (two
    /// at most; a third click starts a new pair).
    pub asm_picks: Vec<AsmPick>,
    /// In an assembly: the point, face or edge under the cursor.
    pub asm_hover: Option<AsmPick>,
    /// Snap points per component, while the window is open.
    snaps: std::cell::RefCell<std::collections::HashMap<crate::document::DocRef, Snaps>>,
    /// The part's snap points and the body they were made for.
    part_snaps: std::cell::RefCell<Option<(String, Snaps)>>,
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
            SelItem::Point(_, b) => out.push(Target::Point(Point3::new(
                f64::from_bits(b[0]),
                f64::from_bits(b[1]),
                f64::from_bits(b[2]),
            ))),
            SelItem::Body(n) => out.push(Target::Body(n.clone())),
            SelItem::Feature(_) | SelItem::Sketch(_) => skipped = true,
        }
    }
    let n = out.len();
    (out.split_off(n.saturating_sub(2)), skipped)
}

fn mm(v: f64) -> String {
    // Never "-0.000": a rounding error is not a direction.
    let v = if v.abs() < 0.0005 { 0.0 } else { v };
    format!("{v:.3} mm")
}

impl CadCraftApp {
    /// Inspect → Measure: opens (or closes) the window.
    pub(super) fn toggle_measure(&mut self) {
        self.measure.open = !self.measure.open;
    }

    /// The right-click menu's Measure: opens the window on the selection.
    pub(super) fn measure_context(&mut self, ui: &mut egui::Ui) {
        if super::icons::menu_item(ui, Some(super::icons::Icon::Measure), "Measure", true)
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
        self.assembly()?;
        let picks = &self.measure.asm_picks;
        if picks.is_empty() {
            return None;
        }
        let mut key = format!("asm|{picks:?}");
        for g in picks {
            let Some((component, t)) = self.asm_pick_part(g) else {
                return Some(Err("the component is gone".into()));
            };
            let sig = self.with_part(component, |p| p.body_signature())?;
            key.push_str(&format!("|{sig}|{t:?}"));
        }
        if let Some((k, r)) = &*self.measure.cache.borrow()
            && *k == key
        {
            return Some(r.clone());
        }
        let mut placed: Vec<(BRep, Target, Transform3)> = Vec::new();
        for g in picks {
            let (component, at) = self.asm_pick_part(g)?;
            let item = self.with_part(component, |p| {
                let t = match g {
                    AsmPick::Point { local, .. } => {
                        Target::Point(Point3::new(local[0], local[1], local[2]))
                    }
                    AsmPick::Geom(GeomRef::Face { name, .. }) => Target::Face(name.clone()),
                    AsmPick::Geom(GeomRef::Edge { edge, .. }) => {
                        Target::Edge(edge_by_ref(&p.body, edge)?)
                    }
                    AsmPick::Geom(_) => return None,
                };
                Some((p.body.clone(), t))
            })?;
            let Some((body, t)) = item else {
                return Some(Err("this item can't be measured".into()));
            };
            placed.push((body, t, at));
        }
        let refs: Vec<measure::Placed> =
            placed.iter().map(|(b, t, m)| (b, t.clone(), *m)).collect();
        let r = measure::measure_placed(&refs);
        *self.measure.cache.borrow_mut() = Some((key, r.clone()));
        Some(r)
    }

    /// The part an assembly pick lies on and its placement now.
    fn asm_pick_part(&self, g: &AsmPick) -> Option<(crate::document::DocRef, Transform3)> {
        match g {
            AsmPick::Geom(x) => self.geom_part(x),
            AsmPick::Point {
                occurrence, sub, ..
            } => self
                .pick_parts()
                .into_iter()
                .find(|p| p.top == *occurrence && p.sub == *sub)
                .map(|p| (p.component, p.place)),
        }
    }

    /// The snap point of any component nearest the cursor. Every part whose
    /// box the eye ray passes is asked, not only the one under the cursor: a
    /// hole's centre has nothing under it. One in front of what the ray hits.
    fn asm_snap_under(&self, p: egui::Pos2, rect: egui::Rect) -> Option<AsmPick> {
        let (o, d) = self.camera.ray(p, rect)?;
        let d = d.try_normalize(1e-12)?;
        let front = self.part_hit(p, rect).map(|h| h.2);
        let mut best: Option<(f32, AsmPick)> = None;
        for part in self.pick_parts() {
            let cached = self.measure.snaps.borrow().get(&part.component).cloned();
            let snaps = match cached {
                Some(s) => s,
                None => {
                    let s = self.with_part(part.component, |x| snaps_of(&x.body))?;
                    self.measure
                        .snaps
                        .borrow_mut()
                        .insert(part.component, s.clone());
                    s
                }
            };
            let inv = part.place.inverse();
            if !crate::assembly::select::ray_hits_box(&(inv * o), &(inv * d), &snaps.1, &snaps.2) {
                continue;
            }
            for (kind, at) in &snaps.0 {
                let w = part.place * at;
                let Some(s) = self.camera.project(&w, rect) else {
                    continue;
                };
                let dist = s.distance(p);
                if dist > snap_radius(*kind)
                    || best.as_ref().is_some_and(|b| b.0 <= dist)
                    || front.is_some_and(|t| (w - o).dot(&d) > t * 1.02)
                {
                    continue;
                }
                best = Some((
                    dist,
                    AsmPick::Point {
                        occurrence: part.top,
                        sub: part.sub.clone(),
                        kind: *kind,
                        local: [at.x, at.y, at.z],
                    },
                ));
            }
        }
        best.map(|b| b.1)
    }

    /// The point, face or edge Measure takes at `p`: a point first, so two
    /// corners, middles or centres give their distance.
    fn asm_pick_under(&self, p: egui::Pos2, rect: egui::Rect) -> Option<AsmPick> {
        self.asm_snap_under(p, rect)
            .or_else(|| self.geom_under_pub(p, rect).map(AsmPick::Geom))
    }

    /// Draws a pick: a dot on a point, the geometry otherwise.
    fn draw_asm_pick(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        g: &AsmPick,
        col: Color32,
        label: bool,
    ) {
        match g {
            AsmPick::Geom(x) => self.draw_geom(painter, rect, x, col),
            AsmPick::Point { kind, local, .. } => {
                let Some((_, place)) = self.asm_pick_part(g) else {
                    return;
                };
                let w = place * Point3::new(local[0], local[1], local[2]);
                if let Some(s) = self.camera.project(&w, rect) {
                    dot_at(painter, s, *kind, col, label);
                }
            }
        }
    }

    /// The middle or centre Measure snaps to in a part (`idle`): vertices come
    /// from the selection itself.
    pub(super) fn measure_snap_at(&self, p: egui::Pos2, rect: egui::Rect) -> Option<SelItem> {
        if !self.measure.open || self.measure.loop_mode {
            return None;
        }
        let key = self.body_key();
        let cached = self
            .measure
            .part_snaps
            .borrow()
            .as_ref()
            .filter(|(k, _)| *k == key)
            .map(|(_, s)| s.clone());
        let snaps = cached.unwrap_or_else(|| {
            let s = snaps_of(&self.part().body);
            *self.measure.part_snaps.borrow_mut() = Some((key, s.clone()));
            s
        });
        let (o, d) = self.camera.ray(p, rect)?;
        let d = d.try_normalize(1e-12)?;
        let front = self.picker().pick(&o, &d).map(|(_, t)| t);
        let mut best: Option<(f32, SelItem)> = None;
        for (kind, at) in &snaps.0 {
            if *kind == SnapKind::Vertex {
                continue;
            }
            let Some(s) = self.camera.project(at, rect) else {
                continue;
            };
            let dist = s.distance(p);
            if dist > snap_radius(*kind)
                || best.as_ref().is_some_and(|b| b.0 <= dist)
                || front.is_some_and(|t| (at - o).dot(&d) > t * 1.02)
            {
                continue;
            }
            best = Some((
                dist,
                SelItem::Point(*kind, [at.x.to_bits(), at.y.to_bits(), at.z.to_bits()]),
            ));
        }
        best.map(|b| b.1)
    }

    /// A part's snapped point in the view.
    pub(super) fn draw_snap_item(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        kind: SnapKind,
        bits: &[u64; 3],
        col: Color32,
    ) {
        let w = Point3::new(
            f64::from_bits(bits[0]),
            f64::from_bits(bits[1]),
            f64::from_bits(bits[2]),
        );
        if let Some(s) = self.camera.project(&w, rect) {
            dot_at(painter, s, kind, col, true);
        }
    }

    /// Measure in an assembly view (no command open): the point, face or edge
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
            self.measure.snaps.borrow_mut().clear();
            return;
        }
        self.measure.asm_hover = pointer.and_then(|p| self.asm_pick_under(p, rect));
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
            self.draw_asm_pick(painter, rect, g, col, false);
        }
        if let Some(g) = self.measure.asm_hover.clone() {
            self.draw_asm_pick(painter, rect, &g, Color32::from_rgb(40, 150, 70), true);
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
            // Its snapped points mean nothing to any other command.
            self.sel.retain(|x| !matches!(x, SelItem::Point(..)));
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
                    // Beside the model it measures, not over it.
                    .pivot(egui::Align2::LEFT_TOP)
                    .default_pos(ctx.content_rect().left_top() + egui::vec2(260.0, 190.0))
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
                        "Click a point, a face or an edge of a component; click a second \
                         one (on the same or another component) for the distance and angle \
                         (a third click starts again)."
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
                        ui.colored_label(super::theme::tokens().warning, format!("Measure: {e}"));
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
                                        for k in 0..3 {
                                            ui.colored_label(
                                                AXIS_COLORS[k],
                                                ["dX", "dY", "dZ"][k],
                                            );
                                            ui.colored_label(AXIS_COLORS[k], mm(v[k]));
                                            ui.end_row();
                                        }
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
        // The way along the axes under the shortest line, each in its colour.
        for (axis, from, to, len) in measure::axis_legs(&p, &q, 1e-6) {
            let (Some(s), Some(e)) = (
                self.camera.project(&from, rect),
                self.camera.project(&to, rect),
            ) else {
                continue;
            };
            let c = AXIS_COLORS[axis];
            painter.line_segment([s, e], Stroke::new(2.0, c));
            value_plate(
                painter,
                s + (e - s) * 0.5,
                format!("{} {len:.3}", ["X", "Y", "Z"][axis]),
                c,
            );
        }
        let col = Color32::from_rgb(230, 120, 20);
        painter.line_segment([a, b], Stroke::new(2.0, col));
        painter.circle_filled(a, 3.5, col);
        painter.circle_filled(b, 3.5, col);
        value_plate(painter, a + (b - a) * 0.5, mm(d), col);
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
