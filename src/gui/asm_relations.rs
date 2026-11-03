//! Assemble › Relationships: Show, Show Sick and Hide All, the glyphs of relationships in
//! the view (click selects, double-click edits, right click for its commands), and
//! View › Degrees of Freedom, the free motions of each component as arrows and arcs
//! (`docs/spec/3.1-assemblies.md`, §3.2b).

use std::collections::HashMap;

use eframe::egui::{self, Align2, FontId, Pos2, Rect, Stroke, Vec2};

use super::CadCraftApp;
use super::assembly::AsmTool;
use crate::assembly::relation::{self, ConstraintKind, Geom, GeomRef};
use crate::assembly::solve::Freedom;
use crate::kernel::{Point3, Real, Transform3, Vector3};

/// Half the size of a glyph on screen.
const GLYPH: Vec2 = Vec2::new(10.0, 8.0);
/// A free slide's arrow, from the component's centre (pixels).
const ARROW_PX: f32 = 30.0;
/// A free turn's arc (pixels).
const ARC_PX: f32 = 15.0;

/// What the view shows of the relationships and of the free motions.
#[derive(Default)]
pub(super) struct RelGlyphs {
    /// The components whose relationships are shown.
    of: Vec<u32>,
    /// The sick relationships are shown.
    pub sick: bool,
    /// The free motions of the components are shown.
    pub dof: bool,
    /// The glyphs as drawn last (for the pointer).
    spots: Vec<(Rect, u32)>,
    /// The geometry of a relationship's end: by (relationship, second end), with the
    /// part's signature and the reference it was resolved from. Finding a face's centre
    /// walks its triangles: not for every frame (rule 20).
    local: HashMap<(u32, bool), (String, String, Geom)>,
}

impl RelGlyphs {
    /// Any relationship shown.
    pub fn shown(&self) -> bool {
        !self.of.is_empty() || self.sick
    }
}

/// The letters on a relationship's glyph.
fn letters(kind: ConstraintKind) -> &'static str {
    use ConstraintKind::*;
    match kind {
        Mate => "M",
        Flush => "F",
        Angle => "A",
        Tangent => "T",
        Insert => "I",
        Symmetry => "S",
        Rigid => "Rg",
        Rotational => "Ro",
        Slider => "Sl",
        Cylindrical => "Cy",
        Planar => "Pl",
        Ball => "Ba",
    }
}

/// A point of the geometry, where its glyph stands.
fn anchor(g: Geom) -> Point3 {
    match g {
        Geom::Plane { p, .. }
        | Geom::Axis { p, .. }
        | Geom::Point { p }
        | Geom::Cylinder { p, .. }
        | Geom::Frame { p, .. } => p,
        Geom::Circle { c, .. } | Geom::Sphere { c, .. } => c,
    }
}

/// A line with an arrow head at `to`.
fn arrow_to(painter: &egui::Painter, from: Pos2, to: Pos2, stroke: Stroke) {
    painter.line_segment([from, to], stroke);
    if (to - from).length() < 1.0 {
        return;
    }
    let d = (to - from).normalized();
    let side = Vec2::new(-d.y, d.x);
    for s in [1.0, -1.0] {
        painter.line_segment([to, to - d * 6.0 + side * s * 3.5], stroke);
    }
}

impl CadCraftApp {
    /// Assemble › Show: the relationships of the selected components; with none selected
    /// the command waits for clicks on components.
    pub(super) fn show_relationships(&mut self) {
        if self.asm.tool == Some(AsmTool::ShowRel) {
            self.asm.tool = None;
            return;
        }
        let sel = self.asm.sel.clone();
        if sel.is_empty() {
            self.asm.tool = Some(AsmTool::ShowRel);
            return;
        }
        for id in sel {
            self.show_relationships_of(id, false);
        }
    }

    /// Shows the relationships of component `id`; `toggle`: a second click takes them away.
    pub(super) fn show_relationships_of(&mut self, id: u32, toggle: bool) {
        let of = &mut self.cons.glyphs.of;
        match of.iter().position(|x| *x == id) {
            Some(k) if toggle => {
                of.remove(k);
                return;
            }
            Some(_) => return,
            None => of.push(id),
        }
        let Some(a) = self.assembly() else {
            return;
        };
        if !a.constraints.iter().any(|c| c.uses(id)) {
            let name = a.find(id).map_or("?", |o| o.name.as_str()).to_owned();
            self.file_status = Some(Ok(format!("Show: {name} has no relationships")));
        }
    }

    /// Assemble › Show Sick: on or off.
    pub(super) fn show_sick_relationships(&mut self) {
        let on = !self.cons.glyphs.sick;
        self.cons.glyphs.sick = on;
        if on {
            let n = self.cons.last.sick.len();
            self.file_status = Some(if n == 0 {
                Ok("Show Sick: no relationship is sick".into())
            } else {
                Err(format!("Show Sick: {n} sick relationship(s), in red"))
            });
        }
    }

    /// Assemble › Hide All.
    pub(super) fn hide_relationships(&mut self) {
        self.cons.glyphs.of.clear();
        self.cons.glyphs.sick = false;
        if self.asm.tool == Some(AsmTool::ShowRel) {
            self.asm.tool = None;
        }
    }

    /// The relationship whose glyph is under `p`.
    pub(super) fn relation_glyph_at(&self, p: Pos2) -> Option<u32> {
        self.cons
            .glyphs
            .spots
            .iter()
            .find(|(r, _)| r.contains(p))
            .map(|(_, id)| *id)
    }

    /// A click on a glyph: the relationship is selected (its Properties show).
    pub(super) fn relation_glyph_select(&mut self, id: u32) {
        self.cons.selected = Some(id);
        self.cons.more.clear();
        self.asm.sel.clear();
        self.asm.sub.clear();
    }

    /// The right-click menu of a glyph.
    pub(super) fn relation_glyph_menu(&mut self, ui: &mut egui::Ui, id: u32) {
        let suppressed = self
            .assembly()
            .and_then(|a| a.constraint(id))
            .is_some_and(|c| c.suppressed);
        if ui.button("Edit").clicked() {
            self.edit_constraint(id);
            ui.close();
        }
        if ui
            .button(if suppressed { "Unsuppress" } else { "Suppress" })
            .clicked()
        {
            self.toggle_constraint_suppress(id);
            ui.close();
        }
        if ui.button("Delete").clicked() {
            self.delete_constraint(id);
            ui.close();
        }
        ui.separator();
        if ui.button("Hide All").clicked() {
            self.hide_relationships();
            ui.close();
        }
    }

    /// Where the end `second` of relationship `id` stands in the assembly.
    fn relation_end(&mut self, id: u32, second: bool, r: &GeomRef) -> Option<Point3> {
        let t = if r.occurrence() == relation::ASSEMBLY {
            Transform3::identity()
        } else {
            self.assembly()?.find(r.occurrence())?.transform
        };
        let signature = match self.geom_part(r) {
            Some((component, _)) => self
                .with_part(component, |p| p.body_signature())
                .unwrap_or_default(),
            None => String::new(),
        };
        let label = r.label();
        let cached = self.cons.glyphs.local.get(&(id, second));
        if let Some((s, l, g)) = cached
            && *s == signature
            && *l == label
        {
            return Some(anchor(g.clone().moved(&t)));
        }
        let g = self.asm_local_geom(r).ok()?;
        self.cons
            .glyphs
            .local
            .insert((id, second), (signature, label, g.clone()));
        Some(anchor(g.moved(&t)))
    }

    /// The glyphs of the relationships shown: one at each end, joined by a thin line; the
    /// geometry and a note under the pointer.
    pub(super) fn relation_glyphs(
        &mut self,
        painter: &egui::Painter,
        rect: Rect,
        pointer: Option<Pos2>,
    ) {
        self.cons.glyphs.spots.clear();
        if !self.cons.glyphs.shown() {
            return;
        }
        let Some(a) = self.assembly() else {
            return;
        };
        let sick: HashMap<u32, String> = self.cons.last.sick.iter().cloned().collect();
        let (of, with_sick) = (self.cons.glyphs.of.clone(), self.cons.glyphs.sick);
        let list: Vec<relation::Constraint> = a
            .constraints
            .iter()
            .filter(|c| of.iter().any(|o| c.uses(*o)) || (with_sick && sick.contains_key(&c.id)))
            .cloned()
            .collect();
        let colours = super::theme::canvas();
        let mut spots: Vec<(Rect, u32)> = Vec::new();
        let mut under = None;
        for c in &list {
            let mut ends: Vec<Pos2> = Vec::new();
            for (second, r) in [(false, &c.a), (true, &c.b)] {
                if let Some(p) = self.relation_end(c.id, second, r) {
                    ends.extend(self.camera.project(&p, rect));
                }
            }
            // Geometry that is gone: the glyph on the component.
            if ends.is_empty()
                && let Some((lo, hi)) = self.asm_box(c.a.occurrence())
            {
                let middle = Point3::from((lo.coords + hi.coords) / 2.0);
                ends.extend(self.camera.project(&middle, rect));
            }
            let colour = if sick.contains_key(&c.id) {
                colours.sketch_failed
            } else if self.cons.selected == Some(c.id) {
                colours.highlight
            } else if c.suppressed {
                colours.reference
            } else {
                colours.dimension
            };
            if let [p, q] = ends[..] {
                painter.line_segment([p, q], Stroke::new(1.0, colour));
            }
            for at in ends {
                let mut r = Rect::from_center_size(at, GLYPH * 2.0);
                // Several at one place stand side by side.
                while spots.iter().any(|(o, _)| o.intersects(r)) {
                    r = r.translate(Vec2::new(GLYPH.x * 2.0 + 2.0, 0.0));
                }
                painter.rect(
                    r,
                    3.0,
                    colours.background_top,
                    Stroke::new(1.5, colour),
                    egui::StrokeKind::Middle,
                );
                painter.text(
                    r.center(),
                    Align2::CENTER_CENTER,
                    letters(c.kind),
                    FontId::proportional(11.0),
                    colour,
                );
                spots.push((r, c.id));
                if pointer.is_some_and(|p| r.contains(p)) {
                    under = Some(c.clone());
                }
            }
        }
        self.cons.glyphs.spots = spots;
        if let (Some(c), Some(p)) = (under, pointer) {
            self.draw_geom(painter, rect, &c.a, colours.highlight);
            self.draw_geom(painter, rect, &c.b, colours.highlight);
            let state = match sick.get(&c.id) {
                Some(why) => format!(": sick, {why}"),
                None if c.suppressed => " (suppressed)".to_owned(),
                None => String::new(),
            };
            let text = format!("{}{state}", c.name);
            if !painter.ctx().any_popup_open() {
                super::command_ui::cursor_hint(painter, rect, p, &text);
            }
        }
    }

    /// View › Degrees of Freedom: from each free component's centre an arrow per free slide
    /// and an arc per free turn.
    pub(super) fn dof_glyphs(&mut self, painter: &egui::Painter, rect: Rect) {
        // A preview shows the components elsewhere: the glyphs would stand where they were.
        if !self.cons.glyphs.dof || self.cons.preview.is_some() {
            return;
        }
        let Some(a) = self.assembly() else {
            return;
        };
        let colours = super::theme::canvas();
        let (slide, turn) = (
            Stroke::new(2.0, colours.dimension),
            Stroke::new(2.0, colours.highlight),
        );
        // No relationships: everything is free, and nothing was solved to say so.
        let free_all = a.constraints.is_empty();
        let list: Vec<(u32, Freedom)> = a
            .occurrences
            .iter()
            .filter(|o| o.visible && !o.suppressed && !o.grounded && !a.is_element(o.id))
            .filter_map(|o| {
                let free = self.cons.last.free.get(&o.id).cloned();
                free.or_else(|| free_all.then(Freedom::all))
                    .map(|f| (o.id, f))
            })
            .collect();
        for (id, f) in list {
            let Some((lo, hi)) = self.asm_box(id) else {
                continue;
            };
            let middle = Point3::from((lo.coords + hi.coords) / 2.0);
            let Some(c) = self.camera.project(&middle, rect) else {
                continue;
            };
            // Pixels per millimetre here.
            let scale = [Vector3::x(), Vector3::y(), Vector3::z()]
                .iter()
                .filter_map(|v| self.camera.project(&(middle + v), rect))
                .map(|s| (s - c).length())
                .fold(0.0f32, f32::max);
            if scale <= 0.0 {
                continue;
            }
            for v in &f.moves {
                let Some(tip) = self.camera.project(&(middle + v / Real::from(scale)), rect) else {
                    continue;
                };
                // Along the line of sight, a ring.
                if (tip - c).length() > 0.15 {
                    let d = (tip - c).normalized();
                    arrow_to(painter, c, c + d * ARROW_PX, slide);
                    arrow_to(painter, c, c - d * ARROW_PX, slide);
                } else {
                    painter.circle_stroke(c, 4.0, slide);
                }
            }
            let radius = Real::from(ARC_PX / scale);
            for axis in &f.turns {
                let side = if axis.x.abs() < 0.9 {
                    Vector3::x()
                } else {
                    Vector3::y()
                };
                let u = axis.cross(&side).normalize();
                let w = axis.cross(&u);
                // Stands out along its axis, so that three arcs do not lie on each other.
                let at = middle + axis * (radius * 1.6);
                let line: Vec<Pos2> = (0..=24)
                    .filter_map(|k| {
                        let t = (20.0 + Real::from(k) * 12.5f64).to_radians();
                        let q = at + (u * t.cos() + w * t.sin()) * radius;
                        self.camera.project(&q, rect)
                    })
                    .collect();
                if let [.., p, q] = line[..] {
                    painter.add(egui::Shape::line(line.clone(), turn));
                    arrow_to(painter, p, q, turn);
                }
            }
        }
    }
}
