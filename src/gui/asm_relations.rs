//! Assemble › Relationships: Show, Show Sick and Hide All, the glyphs of relationships in
//! the view (click selects, double-click edits, right click for its commands), and
//! View › Degrees of Freedom, the free motions of each component as arrows and arcs
//! (`docs/spec/3.1-assemblies.md`, §3.2b). Show Constraints / Show Joints switch on the
//! glyphs of every constraint / joint (and of the frame members); a selected glyph's value
//! is edited next to it and a joint's glyph is dragged along the joint's free motion.

use std::collections::HashMap;

use eframe::egui::{self, Align2, FontId, Pos2, Rect, Stroke, Vec2};

use super::CadCraftApp;
use super::assembly::AsmTool;
use super::valuefield::ExprValue;
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
    /// Show Constraints: every constraint (and frame member) is shown.
    pub all: bool,
    /// Show Joints: every joint is shown.
    pub joints: bool,
    /// The glyphs as drawn last (for the pointer).
    spots: Vec<(Rect, u32)>,
    /// The frame members' glyphs as drawn last (frame document ids).
    members: Vec<(Rect, u32)>,
    /// Where the selected relationship's value box stands (by the glyph drawn last).
    value_at: Option<(u32, Pos2)>,
    /// A joint's glyph being dragged.
    drag: Option<GlyphDrag>,
    /// The geometry of a relationship's end: by (relationship, second end), with the
    /// part's signature and the reference it was resolved from. Finding a face's centre
    /// walks its triangles: not for every frame (rule 20).
    local: HashMap<(u32, bool), (String, String, Geom)>,
}

/// A drag of a joint's glyph: its value follows the pointer.
#[derive(Clone, Copy, Debug)]
struct GlyphDrag {
    id: u32,
    /// The value when the drag started.
    start: Real,
    /// The last value that solved.
    good: Real,
    press: Pos2,
    /// A slide: the joint's axis on the screen (unit) and pixels per millimetre;
    /// `None`: a turn, by the drag sideways.
    slide: Option<(Vec2, f32)>,
    /// Was the value held before the drag?
    was_locked: bool,
}

/// Degrees a pixel of sideways drag turns a Rotational joint.
const TURN_PER_PX: Real = 1.0;

impl RelGlyphs {
    /// Any relationship shown.
    pub fn shown(&self) -> bool {
        !self.of.is_empty() || self.sick || self.all || self.joints
    }

    /// Is relationship `c` shown (`sick`: it is sick now)?
    fn shows(&self, c: &relation::Constraint, sick: bool) -> bool {
        self.of.iter().any(|o| c.uses(*o))
            || (self.sick && sick)
            || (self.all && !c.kind.is_joint())
            || (self.joints && c.kind.is_joint())
    }
}

/// Whether a relationship's value is an angle (joints and Mates of axes measure a turn).
fn is_angle(kind: ConstraintKind, axes: bool) -> bool {
    axes || matches!(kind, ConstraintKind::Angle | ConstraintKind::Rotational)
}

/// Does `c` have a value to show and type next to its glyph?
fn has_value(c: &relation::Constraint, axes: bool) -> bool {
    if c.kind.is_joint() {
        c.kind.joint_value().is_some()
    } else {
        !axes || c.locked
    }
}

/// `c` with Flip applied: Mate and Flush swap, Insert and joints swap Opposed and Aligned;
/// `None` where nothing flips.
pub(super) fn flipped(c: &relation::Constraint) -> Option<relation::Constraint> {
    use ConstraintKind::*;
    let mut f = c.clone();
    match c.kind {
        Mate => f.kind = Flush,
        Flush => f.kind = Mate,
        Insert | Rigid | Rotational | Slider | Cylindrical | Planar => f.aligned = !c.aligned,
        Angle | Tangent | Symmetry | Ball => return None,
    }
    Some(f)
}

/// A joint's value after a drag of `delta` pixels, clamped to its limits.
fn dragged_value(d: &GlyphDrag, delta: Vec2, min: Option<Real>, max: Option<Real>) -> Real {
    let v = match d.slide {
        Some((dir, px_per_mm)) => d.start + Real::from(delta.dot(dir) / px_per_mm.max(1e-6)),
        None => d.start + Real::from(delta.x) * TURN_PER_PX.to_radians(),
    };
    let v = min.map_or(v, |m| v.max(m));
    max.map_or(v, |m| v.min(m))
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
        self.cons.glyphs.all = false;
        self.cons.glyphs.joints = false;
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

    /// A click on a glyph: the relationship is selected (its Properties and browser row
    /// follow); with Ctrl or Shift it is added to the selection or taken out of it.
    pub(super) fn relation_glyph_select(&mut self, id: u32, add: bool) {
        self.asm.sel.clear();
        self.asm.sub.clear();
        if add && self.cons.selected.is_some() {
            if self.cons.selected == Some(id) {
                self.cons.selected = self.cons.more.pop();
            } else if let Some(k) = self.cons.more.iter().position(|x| *x == id) {
                self.cons.more.remove(k);
            } else {
                self.cons.more.push(id);
            }
            return;
        }
        self.cons.selected = Some(id);
        self.cons.more.clear();
    }

    /// Show Constraints (`joints` false) or Show Joints: on or off.
    pub(super) fn toggle_glyphs(&mut self, joints: bool) {
        let g = &mut self.cons.glyphs;
        let on = if joints {
            g.joints = !g.joints;
            g.joints
        } else {
            g.all = !g.all;
            g.all
        };
        if !on {
            return;
        }
        let n = self.assembly().map_or(0, |a| {
            a.constraints
                .iter()
                .filter(|c| c.kind.is_joint() == joints)
                .count()
        });
        let what = if joints { "joint" } else { "constraint" };
        self.file_status = Some(Ok(if n == 0 {
            format!("Show {what}s: the assembly has no {what}s")
        } else {
            format!("Show {what}s: {n} {what}(s); drag, click or right click a glyph")
        }));
    }

    /// The frame member whose glyph is under `p` (an id of the frame's document).
    pub(super) fn member_glyph_at(&self, p: Pos2) -> Option<u32> {
        self.cons
            .glyphs
            .members
            .iter()
            .find(|(r, _)| r.contains(p))
            .map(|(_, id)| *id)
    }

    /// A click on a frame member's glyph: the member is selected.
    pub(super) fn member_glyph_select(&mut self, id: u32) {
        self.cons.selected = None;
        self.cons.more.clear();
        self.frame_set_selected(vec![id]);
    }

    /// The value relationship `c` has as the components stand (the cached ends'
    /// geometry, no face walk): an offset, an angle, a joint's turn or slide.
    fn relation_value_now(&mut self, c: &relation::Constraint) -> Option<Real> {
        let a = self.relation_world(c.id, false, &c.a)?;
        let b = self.relation_world(c.id, true, &c.b)?;
        let (a, b) = (
            relation::for_kind(c.kind, &c.a, a),
            relation::for_kind(c.kind, &c.b, b),
        );
        if relation::is_axis_mate(c.kind, &a, &b) {
            return relation::measure(c, &a.as_frame(), &b.as_frame());
        }
        relation::measure(c, &a, &b)
    }

    /// Is `c` a Mate / Flush of axes (its value is the turn about them)?
    fn relation_axes(&mut self, c: &relation::Constraint) -> bool {
        let (Some(a), Some(b)) = (
            self.relation_local(c.id, false, &c.a),
            self.relation_local(c.id, true, &c.b),
        ) else {
            return false;
        };
        relation::is_axis_mate(
            c.kind,
            &relation::for_kind(c.kind, &c.a, a.0),
            &relation::for_kind(c.kind, &c.b, b.0),
        )
    }

    /// Lock / Unlock of a joint's value or a Mate of axes' turn: locking holds the value
    /// as the components stand.
    pub(super) fn toggle_relation_lock(&mut self, id: u32) {
        let Some(c) = self.assembly().and_then(|a| a.constraint(id)).cloned() else {
            return;
        };
        let now = self.relation_value_now(&c).unwrap_or(c.value);
        let lock = !c.locked;
        let label = if lock { "Lock" } else { "Unlock" };
        if let Err(e) = self.asm_try(label, |a| {
            if let Some(x) = a.constraint_mut(id) {
                x.locked = lock;
                if lock {
                    x.value = now;
                    x.expr = None;
                }
            }
        }) {
            self.file_status = Some(Err(format!("{label}: {e}")));
        }
    }

    /// Flip (see [`flipped`]), solved on a copy (rule 5).
    pub(super) fn flip_relation(&mut self, id: u32) {
        let Some(f) = self
            .assembly()
            .and_then(|a| a.constraint(id))
            .and_then(flipped)
        else {
            return;
        };
        if let Err(e) = self.asm_try("Flip", |a| {
            if let Some(x) = a.constraint_mut(id) {
                *x = f;
            }
        }) {
            self.file_status = Some(Err(format!("Flip: {e}")));
        }
    }

    /// The value typed next to a glyph (internal units): a held value is set; a free
    /// joint moves there and stays free. Solved on a copy (rule 5).
    fn set_relation_value(&mut self, id: u32, value: Real, expr: Option<String>) {
        let Some(c) = self.assembly().and_then(|a| a.constraint(id)).cloned() else {
            return;
        };
        let free = c.kind.is_joint() && !c.locked;
        let r = self.asm_try("Edit Constraint", |a| {
            if let Some(x) = a.constraint_mut(id) {
                x.value = value;
                x.expr = if free { None } else { expr };
                x.locked = x.locked || free;
            }
        });
        match r {
            Err(e) => self.file_status = Some(Err(format!("{}: {e}", c.kind.label()))),
            Ok(()) if free => {
                // Moved there; the motion stays free (no second solve: it holds).
                if let Some(x) = self.assembly_mut().and_then(|a| a.constraint_mut(id)) {
                    x.locked = false;
                }
                self.asm_refresh_status();
            }
            Ok(()) => {}
        }
    }

    /// The box next to the selected glyph with its value (the sketch's field, rule 13).
    pub(super) fn relation_value_box(&mut self, ctx: &egui::Context) {
        let Some((id, at)) = self.cons.glyphs.value_at else {
            return;
        };
        if self.cons.glyphs.drag.is_some() || self.cons.draft.is_some() {
            return;
        }
        let Some(c) = self.assembly().and_then(|a| a.constraint(id)).cloned() else {
            return;
        };
        let axes = self.relation_axes(&c);
        if !has_value(&c, axes) {
            return;
        }
        let angle = is_angle(c.kind, axes);
        let held = c.holds_value() || (axes && c.locked);
        let now = if held {
            c.value
        } else {
            self.relation_value_now(&c).unwrap_or(c.value)
        };
        let mut v = if angle { now.to_degrees() } else { now };
        let before = v;
        let mut expr = if held { c.expr.clone() } else { None };
        let label = match c.kind.joint_value() {
            Some(w) => w,
            None if angle => "Angle",
            None => "Offset",
        };
        egui::Area::new(egui::Id::new(("rel_value", id)))
            .fixed_pos(at)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(label).small());
                        ui.add(
                            ExprValue::new(&mut v)
                                .suffix(if angle { "°" } else { " mm" })
                                .equation(&mut expr),
                        );
                    });
                    if !held {
                        ui.label(egui::RichText::new("free: typed, it moves there").small());
                    }
                });
            });
        if v != before {
            let value = if angle { v.to_radians() } else { v };
            self.set_relation_value(id, value, expr);
        }
    }

    /// A drag of a joint's glyph moves the joint along its free motion (`true` while it
    /// is held: the view neither orbits nor box-selects). Solved once a frame, as a
    /// component drag; one Undo step.
    pub(super) fn relation_glyph_grip(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        rect: Rect,
    ) -> bool {
        if response.drag_started_by(egui::PointerButton::Primary)
            && self.cons.draft.is_none()
            && let Some(press) = ui.input(|i| i.pointer.press_origin())
            && let Some(id) = self.relation_glyph_at(press)
        {
            return self.glyph_drag_start(id, press, rect);
        }
        let Some(d) = self.cons.glyphs.drag else {
            return false;
        };
        if !response.dragged() {
            self.cons.glyphs.drag = None;
            if let Some(x) = self.assembly_mut().and_then(|a| a.constraint_mut(d.id)) {
                x.locked = d.was_locked;
                if !d.was_locked {
                    x.value = d.start;
                }
            }
            self.history.break_coalescing();
            self.asm_solve(None);
            return false;
        }
        let Some(now) = response.interact_pointer_pos() else {
            return true;
        };
        let Some((min, max)) = self
            .assembly()
            .and_then(|a| a.constraint(d.id))
            .map(|c| (c.min, c.max))
        else {
            return true;
        };
        let v = dragged_value(&d, now - d.press, min, max);
        let set = |app: &mut Self, v: Real| {
            if let Some(x) = app.assembly_mut().and_then(|a| a.constraint_mut(d.id)) {
                x.value = v;
                x.locked = true;
            }
            app.asm_solve_once(None);
        };
        set(self, v);
        if self.cons.last.converged {
            if let Some(g) = self.cons.glyphs.drag.as_mut() {
                g.good = v;
            }
            let shown = if d.slide.is_some() {
                format!("{v:.2} mm")
            } else {
                format!("{:.1}°", v.to_degrees())
            };
            self.file_status = Some(Ok(format!("Drag: {shown}")));
        } else {
            // Something else holds it: back to the last value that solved.
            set(self, d.good);
        }
        self.scene_dirty = true;
        true
    }

    /// Starts the drag of joint `id`'s glyph pressed at `press`; `false` when the joint
    /// has no motion to drag.
    fn glyph_drag_start(&mut self, id: u32, press: Pos2, rect: Rect) -> bool {
        let Some(c) = self.assembly().and_then(|a| a.constraint(id)).cloned() else {
            return false;
        };
        if c.suppressed || !c.kind.is_joint() {
            return false;
        }
        let slide = match c.kind.joint_value() {
            Some("Angle") => None,
            Some(_) => {
                let Some(g) = self.relation_world(id, false, &c.a) else {
                    return false;
                };
                let f = crate::assembly::joint::frame_of(&g);
                let (Some(p), Some(q)) = (
                    self.camera.project(&f.p, rect),
                    self.camera.project(&(f.p + f.z), rect),
                ) else {
                    return false;
                };
                let px = (q - p).length();
                // Along the line of sight: a drag upwards slides it out.
                if px < 0.05 {
                    Some((Vec2::new(0.0, -1.0), 1.0))
                } else {
                    Some(((q - p) / px, px))
                }
            }
            None => {
                self.file_status = Some(Err(format!(
                    "Drag: a {} joint has no value to drive",
                    c.kind.label()
                )));
                return false;
            }
        };
        let start = if c.locked {
            c.value
        } else {
            self.relation_value_now(&c).unwrap_or(c.value)
        };
        self.checkpoint("Drag Joint");
        self.cons.selected = Some(id);
        self.cons.more.clear();
        self.cons.glyphs.drag = Some(GlyphDrag {
            id,
            start,
            good: start,
            press,
            slide,
            was_locked: c.locked,
        });
        true
    }

    /// The right-click menu of a glyph.
    pub(super) fn relation_glyph_menu(&mut self, ui: &mut egui::Ui, id: u32) {
        let Some(c) = self.assembly().and_then(|a| a.constraint(id)).cloned() else {
            return;
        };
        let suppressed = c.suppressed;
        let axes = self.relation_axes(&c);
        if ui.button("Edit").clicked() {
            self.edit_constraint(id);
            ui.close();
        }
        if ((c.kind.is_joint() && c.kind.joint_value().is_some()) || axes)
            && ui
                .button(if c.locked { "Unlock" } else { "Lock" })
                .clicked()
        {
            self.toggle_relation_lock(id);
            ui.close();
        }
        if has_value(&c, axes) || c.kind.joint_value().is_some() {
            if ui.button("Limits...").clicked() {
                self.edit_constraint(id);
                ui.close();
            }
            if ui.button("Drive...").clicked() {
                self.open_drive(id);
                ui.close();
            }
        }
        if flipped(&c).is_some() && ui.button("Flip").clicked() {
            self.flip_relation(id);
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
        self.relation_world(id, second, r).map(anchor)
    }

    /// The geometry of the end `second` of relationship `id` in the assembly.
    fn relation_world(&mut self, id: u32, second: bool, r: &GeomRef) -> Option<Geom> {
        let (g, t) = self.relation_local(id, second, r)?;
        Some(g.moved(&t))
    }

    /// The geometry of the end `second` of relationship `id` in its occurrence (cached by
    /// the part's signature) and that occurrence's placement.
    fn relation_local(&mut self, id: u32, second: bool, r: &GeomRef) -> Option<(Geom, Transform3)> {
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
            return Some((*g, t));
        }
        let g = self.asm_local_geom(r).ok()?;
        self.cons
            .glyphs
            .local
            .insert((id, second), (signature, label, g));
        Some((g, t))
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
        self.cons.glyphs.members.clear();
        self.cons.glyphs.value_at = None;
        if !self.cons.glyphs.shown() {
            return;
        }
        let Some(a) = self.assembly() else {
            return;
        };
        let sick: HashMap<u32, String> = self.cons.last.sick.iter().cloned().collect();
        let glyphs = &self.cons.glyphs;
        let list: Vec<relation::Constraint> = a
            .constraints
            .iter()
            .filter(|c| glyphs.shows(c, sick.contains_key(&c.id)))
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
            let selected = self.cons.selected == Some(c.id) || self.cons.more.contains(&c.id);
            let colour = if sick.contains_key(&c.id) {
                colours.sketch_failed
            } else if selected {
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
                if self.cons.selected == Some(c.id) && self.cons.glyphs.value_at.is_none() {
                    self.cons.glyphs.value_at = Some((c.id, r.right_top() + Vec2::new(4.0, 0.0)));
                }
                if pointer.is_some_and(|p| r.contains(p)) {
                    under = Some(c.clone());
                }
            }
        }
        let member_under = self.member_glyphs(painter, rect, pointer, &spots);
        self.cons.glyphs.spots = spots;
        if let (Some(text), Some(p)) = (member_under, pointer)
            && !painter.ctx().any_popup_open()
        {
            super::command_ui::cursor_hint(painter, rect, p, &text);
        }
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

    /// Show Constraints: a `Fr` glyph at the middle of each frame member's skeleton line
    /// (the relationship that holds it); under the pointer the line lights up. Returns the
    /// hint of the glyph under the pointer.
    fn member_glyphs(
        &mut self,
        painter: &egui::Painter,
        rect: Rect,
        pointer: Option<Pos2>,
        taken: &[(Rect, u32)],
    ) -> Option<String> {
        if !self.cons.glyphs.all {
            return None;
        }
        let lines = self.frame_member_lines();
        if lines.is_empty() {
            return None;
        }
        let colours = super::theme::canvas();
        let selected = self.frame_selected();
        let mut members: Vec<(Rect, u32)> = Vec::new();
        let mut under = None;
        for (m, (p, q)) in lines {
            let middle = Point3::from((p.coords + q.coords) / 2.0);
            let Some(at) = self.camera.project(&middle, rect) else {
                continue;
            };
            let mut r = Rect::from_center_size(at, GLYPH * 2.0);
            while taken.iter().chain(&members).any(|(o, _)| o.intersects(r)) {
                r = r.translate(Vec2::new(GLYPH.x * 2.0 + 2.0, 0.0));
            }
            let hot = pointer.is_some_and(|x| r.contains(x));
            let colour = if selected.contains(&m) || hot {
                colours.highlight
            } else {
                colours.dimension
            };
            if hot
                && let (Some(a), Some(b)) =
                    (self.camera.project(&p, rect), self.camera.project(&q, rect))
            {
                painter.line_segment([a, b], Stroke::new(3.0, colours.highlight));
                painter.circle_filled(a, 3.5, colours.highlight);
                painter.circle_filled(b, 3.5, colours.highlight);
                let name = self
                    .frame_asm()
                    .and_then(|f| f.find(m))
                    .map_or_else(|| "member".to_owned(), |o| o.name.clone());
                under = Some(format!(
                    "{name}: held by its skeleton line (double click: Edit Member)"
                ));
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
                "Fr",
                FontId::proportional(11.0),
                colour,
            );
            members.push((r, m));
        }
        self.cons.glyphs.members = members;
        under
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assembly::relation::{Constraint, GeomRef};

    fn rel(kind: ConstraintKind) -> Constraint {
        let r = GeomRef::Origin {
            occurrence: 1,
            sub: Vec::new(),
            item: crate::assembly::relation::OriginItem::XyPlane,
        };
        Constraint::new(kind, r.clone(), r)
    }

    #[test]
    fn flip_swaps_mate_and_flush_and_turns_joints_round() {
        assert_eq!(
            flipped(&rel(ConstraintKind::Mate)).map(|c| c.kind),
            Some(ConstraintKind::Flush)
        );
        assert_eq!(
            flipped(&rel(ConstraintKind::Flush)).map(|c| c.kind),
            Some(ConstraintKind::Mate)
        );
        let r = flipped(&rel(ConstraintKind::Rotational)).map(|c| c.aligned);
        assert_eq!(r, Some(true));
        assert!(flipped(&rel(ConstraintKind::Angle)).is_none());
        assert!(flipped(&rel(ConstraintKind::Ball)).is_none());
    }

    #[test]
    fn show_switches_pick_constraints_or_joints() {
        let g = RelGlyphs {
            all: true,
            ..Default::default()
        };
        assert!(g.shown());
        assert!(g.shows(&rel(ConstraintKind::Mate), false));
        assert!(!g.shows(&rel(ConstraintKind::Slider), false));
        let j = RelGlyphs {
            joints: true,
            ..Default::default()
        };
        assert!(j.shows(&rel(ConstraintKind::Slider), false));
        assert!(!j.shows(&rel(ConstraintKind::Insert), false));
        let none = RelGlyphs::default();
        assert!(!none.shown());
        assert!(!none.shows(&rel(ConstraintKind::Mate), true));
    }

    #[test]
    fn a_dragged_glyph_slides_along_the_axis_and_stops_at_the_limits() {
        let d = GlyphDrag {
            id: 1,
            start: 10.0,
            good: 10.0,
            press: Pos2::ZERO,
            slide: Some((Vec2::new(1.0, 0.0), 2.0)),
            was_locked: false,
        };
        // 20 px along an axis drawn 2 px a millimetre: 10 mm; sideways moves nothing.
        assert!((dragged_value(&d, Vec2::new(20.0, 7.0), None, None) - 20.0).abs() < 1e-9);
        assert!((dragged_value(&d, Vec2::new(20.0, 0.0), None, Some(15.0)) - 15.0).abs() < 1e-9);
        assert!((dragged_value(&d, Vec2::new(-40.0, 0.0), Some(0.0), None)).abs() < 1e-9);
        let turn = GlyphDrag {
            slide: None,
            start: 0.0,
            ..d
        };
        let v = dragged_value(&turn, Vec2::new(90.0, 30.0), None, None);
        assert!((v - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
    }
}
