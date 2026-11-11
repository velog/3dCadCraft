//! The Shaft generator (spec 6.2-design-calculators §3.1–3.2): a window of its
//! own like Inventor's Shaft Component Generator, with three tabs.
//!
//! - **Design**: the shaft's profile drawn to scale with its dimensions (a
//!   click on one opens its value in place, the expression field with units),
//!   the section list with Insert / Move / Delete, and the selected section's
//!   sizes and features (chamfer, fillet, retaining-ring and relief grooves,
//!   keyway, thread; splines shown, not built yet).
//! - **Calculation**: material, supports and loads along the shaft; results
//!   with units and the cited formulas (NASA RP-1123), recomputed live.
//! - **Graphs**: shear force, bending moment, torque, deflection, stress and
//!   safety along the shaft.
//!
//! The Properties panel keeps the placement (Axis, Start Plane, Flip) and OK.
//! A worker thread builds the part for the 3D preview; OK places it.

use std::sync::{Arc, Mutex};

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Stroke, Vec2};

use super::CadCraftApp;
use super::asm_planned::AsmPlanned;
use super::panels::plabel;
use super::part_planned::{PartDialog, Planned};
use super::valuefield::ExprValue;
use crate::assembly::relation::{ASSEMBLY, Geom};
use crate::design::key;
use crate::design::shaft::{Bore, EdgeFeature, RingGroove, Section, SectionShape, ShaftDesign};
use crate::design::shaft_calc::{
    Load, Material, ReducedStress, Reliability, ShaftLoads, ShaftResults, Support, SupportKind,
    calculate,
};
use crate::document::{DocRef, DocumentData};
use crate::kernel::{Point3, Transform3, Vector3};
use crate::part::PartDocument;

/// The generator window's tab.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    /// Sections and features.
    #[default]
    Design,
    /// Loads, supports, material, results.
    Calculation,
    /// The curves.
    Graphs,
}

/// A size drawn on the profile, editable in place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dim {
    /// A section's length.
    Length(usize),
    /// A section's (first) diameter.
    Diameter(usize),
    /// A cone's second diameter.
    Diameter2(usize),
    /// A chamfer or fillet at a section's start (false) or end (true).
    Edge(usize, bool),
    /// A keyway's length.
    Keyway(usize),
    /// A groove's width.
    GrooveWidth(usize, usize),
    /// A bore's diameter (false: the left one).
    BoreD(bool),
    /// A bore's depth.
    BoreDepth(bool),
}

/// The in-place edit box of a dimension.
#[derive(Clone, Copy, Debug)]
pub struct DimEdit {
    /// Which size.
    pub dim: Dim,
    /// Where the box opens (screen).
    pub at: Pos2,
    /// Frames until the box takes the focus (not in the frame of the click
    /// that opened it: that click would take it away again).
    pub fresh: u8,
}

/// The preview job's state, shared with its worker thread.
#[derive(Debug, Default)]
pub struct PreviewSlot {
    /// The design being built.
    running: Option<ShaftDesign>,
    /// The last built design and its part (or why it failed).
    done: Option<(ShaftDesign, Result<Arc<PartDocument>, String>)>,
    /// Counts finished builds (the scene rebuilds when it changes).
    rev: u64,
}

/// The panel's own state (`PartDialog::shaft`).
#[derive(Clone, Debug)]
pub struct ShaftPanel {
    /// The sections.
    pub design: ShaftDesign,
    /// The section being edited.
    pub selected: usize,
    /// The calculation's inputs.
    pub loads: ShaftLoads,
    /// The material preset (index into `Material::presets`; past the end: typed).
    pub preset: usize,
    /// The last calculation and the inputs it was made with.
    pub results: Option<(ShaftLoads, ShaftDesign, Result<ShaftResults, String>)>,
    /// The window's tab.
    pub tab: Tab,
    /// The window is shown.
    pub open: bool,
    /// A dimension being edited on the picture.
    pub edit: Option<DimEdit>,
    /// The 3D preview's part.
    pub preview: Arc<Mutex<PreviewSlot>>,
    /// The preview revision the scene shows.
    pub seen: u64,
    /// OK pressed in the window.
    pub ok: bool,
    /// Cancel pressed in the window.
    pub cancel: bool,
    /// A section kind waiting for its place in the picture (Insert).
    pub placing: Option<Glyph>,
    /// The section being dragged along the shaft (to reorder it).
    pub drag: Option<usize>,
    /// The toolbar's open choice of edges (Chamfer / Fillet / Relief).
    pub edge_menu: Option<(Glyph, Pos2)>,
    /// Editing a placed shaft: its part document and occurrence (OK replaces
    /// the part; the occurrence stays where it is).
    pub editing: Option<(DocRef, u32)>,
    /// The view was framed on the first preview (it stands above the window).
    pub framed: bool,
}

impl Default for ShaftPanel {
    fn default() -> ShaftPanel {
        let design = ShaftDesign::starter();
        let len = design.length();
        ShaftPanel {
            design,
            selected: 0,
            loads: ShaftLoads {
                supports: vec![
                    Support {
                        z: 20.0,
                        kind: SupportKind::Fixed,
                    },
                    Support {
                        z: len - 30.0,
                        kind: SupportKind::Free,
                    },
                ],
                loads: vec![
                    Load::Radial {
                        z: 130.0,
                        force: 1000.0,
                        angle: 0.0,
                    },
                    Load::Torque {
                        z: 20.0,
                        torque: 50e3,
                    },
                    Load::Torque {
                        z: 130.0,
                        torque: -50e3,
                    },
                ],
                ..ShaftLoads::default()
            },
            preset: 0,
            results: None,
            tab: Tab::Design,
            open: true,
            edit: None,
            preview: Arc::default(),
            seen: 0,
            ok: false,
            cancel: false,
            placing: None,
            drag: None,
            edge_menu: None,
            editing: None,
            framed: false,
        }
    }
}

impl ShaftPanel {
    /// Starts a preview build when the design changed (one job at a time).
    fn follow_preview(&self) {
        let Ok(mut slot) = self.preview.lock() else {
            return;
        };
        let current = slot.done.as_ref().is_some_and(|(d, _)| *d == self.design);
        if current || slot.running.is_some() || self.design.validate().is_err() {
            return;
        }
        slot.running = Some(self.design.clone());
        let design = self.design.clone();
        let shared = self.preview.clone();
        std::thread::spawn(move || {
            let built = crate::design::shaft::part(&design).and_then(|mut p| {
                p.regenerate().map_err(|e| e.to_string())?;
                Ok(Arc::new(p))
            });
            if let Ok(mut slot) = shared.lock() {
                slot.running = None;
                slot.done = Some((design, built));
                slot.rev += 1;
            }
        });
    }

    /// Recalculates when the inputs changed (pure arithmetic, cheap).
    fn follow_results(&mut self) {
        let same = self
            .results
            .as_ref()
            .is_some_and(|(l, d, _)| *l == self.loads && *d == self.design);
        if !same {
            let r = calculate(&self.design, &self.loads);
            self.results = Some((self.loads.clone(), self.design.clone(), r));
        }
    }
}

/// Whether `d` is the Shaft panel.
pub(super) fn is_shaft(d: &PartDialog) -> bool {
    d.kind == Planned::Asm(AsmPlanned::Shaft)
}

/// The Sections row of the Properties panel: a summary, the window's
/// toggle, and the window itself.
pub(super) fn sections_ui(d: &mut PartDialog, ui: &mut egui::Ui) {
    plabel(ui, "");
    ui.vertical(|ui| {
        super::bolted::fit_cell(ui);
        let p = &mut d.shaft;
        ui.label(format!(
            "{} sections, {} mm",
            p.design.sections.len(),
            num(p.design.length())
        ));
        if ui
            .selectable_label(p.open, "Shaft Generator window")
            .on_hover_text("The profile, the sections and the calculation")
            .clicked()
        {
            p.open = !p.open;
        }
        if let Err(e) = p.design.validate() {
            ui.colored_label(super::theme::tokens().error, format!("Shaft: {e}"));
        }
    });
    ui.end_row();
    window(d, ui.ctx());
}

/// The generator window.
fn window(d: &mut PartDialog, ctx: &egui::Context) {
    let p = &mut d.shaft;
    p.follow_preview();
    p.follow_results();
    if !p.open {
        return;
    }
    let mut open = true;
    let screen = ctx.content_rect();
    let (pos, size) = window_place(screen);
    egui::Window::new("Shaft Generator")
        .id(egui::Id::new("shaft_generator_window"))
        .open(&mut open)
        .default_size(size)
        .max_width(size.x)
        .default_pos(pos)
        .resizable(true)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                for (t, name) in [
                    (Tab::Design, "Design"),
                    (Tab::Calculation, "Calculation"),
                    (Tab::Graphs, "Graphs"),
                ] {
                    ui.selectable_value(&mut p.tab, t, name);
                }
            });
            ui.separator();
            egui::Panel::bottom("shaft_generator_footer").show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.add(super::theme::ok_button("OK")).clicked() {
                        p.ok = true;
                    }
                    if ui.button("Cancel").clicked() {
                        p.cancel = true;
                    }
                    ui.label(
                        RichText::new(format!(
                            "Length {} mm · {} sections",
                            num(p.design.length()),
                            p.design.sections.len()
                        ))
                        .weak(),
                    );
                });
            });
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| match p.tab {
                    Tab::Design => design_tab(p, ui),
                    Tab::Calculation => calculation_tab(p, ui),
                    Tab::Graphs => graphs_tab(p, ui),
                });
        });
    if !open {
        p.open = false;
    }
    dim_edit_box(p, ctx);
}

/// Where the window opens: along the bottom of the view, over the lower part
/// of the browser and the view, so the upper half of the view (where the
/// preview stands) stays free and Properties (Axis, Start Plane) stays
/// reachable on the right.
fn window_place(screen: Rect) -> (Pos2, Vec2) {
    let w = (screen.width() - 560.0).clamp(560.0, 840.0);
    let h = (screen.height() * 0.5).clamp(360.0, 560.0);
    let x = screen.left() + 250.0;
    let y = screen.bottom() - h - 60.0;
    (Pos2::new(x, y.max(screen.top() + 120.0)), Vec2::new(w, h))
}

fn num(v: f64) -> String {
    let s = format!("{v:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

// ---------------------------------------------------------------- Design tab

/// The icons of the generator (rule 23: every section kind, feature and tool
/// has its own).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    /// A cylinder section.
    Cylinder,
    /// A cone section.
    Cone,
    /// A polygon section.
    Polygon,
    /// A keyway groove.
    Keyway,
    /// Splines.
    Splines,
    /// A chamfer.
    Chamfer,
    /// A fillet.
    Fillet,
    /// A retaining-ring groove.
    RingGroove,
    /// A relief groove.
    Relief,
    /// A thread.
    Thread,
    /// Delete.
    Delete,
    /// Move left.
    MoveLeft,
    /// Move right.
    MoveRight,
    /// Flip the shaft end for end.
    Flip,
    /// A bore into the left end.
    BoreLeft,
    /// A bore into the right end.
    BoreRight,
}

/// Draws glyph `g` in `r`.
fn glyph(p: &egui::Painter, r: Rect, g: Glyph, col: Color32) {
    let at = |x: f32, y: f32| Pos2::new(r.left() + x * r.width(), r.top() + y * r.height());
    let st = Stroke::new(1.4, col);
    let line = |pts: &[(f32, f32)]| {
        p.add(egui::Shape::line(
            pts.iter().map(|&(x, y)| at(x, y)).collect(),
            st,
        ));
    };
    let closed = |pts: &[(f32, f32)]| {
        p.add(egui::Shape::closed_line(
            pts.iter().map(|&(x, y)| at(x, y)).collect(),
            st,
        ));
    };
    let cyl = || closed(&[(0.08, 0.3), (0.92, 0.3), (0.92, 0.7), (0.08, 0.7)]);
    let accent = super::theme::tokens().accent;
    match g {
        Glyph::Cylinder => cyl(),
        Glyph::Cone => closed(&[(0.08, 0.18), (0.92, 0.36), (0.92, 0.64), (0.08, 0.82)]),
        Glyph::Polygon => {
            let pts: Vec<(f32, f32)> = (0..6)
                .map(|k| {
                    let a = std::f32::consts::TAU * k as f32 / 6.0;
                    (0.5 + 0.42 * a.cos(), 0.5 + 0.42 * a.sin())
                })
                .collect();
            closed(&pts);
        }
        Glyph::Keyway => {
            cyl();
            p.rect_filled(
                Rect::from_two_pos(at(0.32, 0.3), at(0.68, 0.44)),
                1.0,
                accent,
            );
        }
        Glyph::Splines => {
            line(&[(0.08, 0.7), (0.92, 0.7)]);
            line(&[
                (0.08, 0.3),
                (0.2, 0.3),
                (0.2, 0.42),
                (0.34, 0.42),
                (0.34, 0.3),
                (0.48, 0.3),
                (0.48, 0.42),
                (0.62, 0.42),
                (0.62, 0.3),
                (0.76, 0.3),
                (0.76, 0.42),
                (0.92, 0.42),
            ]);
        }
        Glyph::Chamfer => line(&[(0.15, 0.88), (0.15, 0.42), (0.42, 0.15), (0.88, 0.15)]),
        Glyph::Fillet => {
            let mut pts = vec![(0.15, 0.88), (0.15, 0.5)];
            for k in 0..=8 {
                let a = std::f32::consts::PI * (1.0 + 0.5 * k as f32 / 8.0);
                pts.push((0.5 + 0.35 * a.cos(), 0.5 + 0.35 * a.sin()));
            }
            pts.push((0.88, 0.15));
            line(&pts);
        }
        Glyph::RingGroove => {
            line(&[
                (0.08, 0.3),
                (0.4, 0.3),
                (0.4, 0.48),
                (0.6, 0.48),
                (0.6, 0.3),
                (0.92, 0.3),
            ]);
            line(&[(0.08, 0.75), (0.92, 0.75)]);
        }
        Glyph::Relief => line(&[
            (0.08, 0.62),
            (0.42, 0.62),
            (0.42, 0.74),
            (0.56, 0.74),
            (0.56, 0.22),
            (0.92, 0.22),
        ]),
        Glyph::Thread => {
            cyl();
            for k in 0..5 {
                let x = 0.16 + 0.16 * k as f32;
                line(&[(x, 0.7), (x + 0.1, 0.3)]);
            }
        }
        Glyph::Delete => {
            line(&[(0.2, 0.2), (0.8, 0.8)]);
            line(&[(0.8, 0.2), (0.2, 0.8)]);
        }
        Glyph::MoveLeft => {
            line(&[(0.85, 0.5), (0.15, 0.5)]);
            line(&[(0.4, 0.25), (0.15, 0.5), (0.4, 0.75)]);
        }
        Glyph::MoveRight => {
            line(&[(0.15, 0.5), (0.85, 0.5)]);
            line(&[(0.6, 0.25), (0.85, 0.5), (0.6, 0.75)]);
        }
        Glyph::BoreLeft | Glyph::BoreRight => {
            cyl();
            let (a, b) = if g == Glyph::BoreLeft {
                (0.08, 0.55)
            } else {
                (0.45, 0.92)
            };
            p.rect_filled(Rect::from_two_pos(at(a, 0.42), at(b, 0.58)), 0.0, accent);
        }
        Glyph::Flip => {
            line(&[(0.1, 0.35), (0.9, 0.35)]);
            line(&[(0.7, 0.18), (0.9, 0.35), (0.7, 0.52)]);
            line(&[(0.9, 0.68), (0.1, 0.68)]);
            line(&[(0.3, 0.51), (0.1, 0.68), (0.3, 0.85)]);
        }
    }
}

/// An icon button of the toolbar.
fn icon_button(ui: &mut egui::Ui, g: Glyph, tip: &str, enabled: bool, on: bool) -> egui::Response {
    let t = super::theme::tokens();
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(28.0), sense);
    let fill = if on {
        t.selection
    } else if enabled && resp.hovered() {
        t.control_hover
    } else {
        t.control
    };
    let painter = ui.painter();
    painter.rect_filled(rect, 3.0, fill);
    let col = if enabled {
        t.text
    } else {
        t.text_weak.gamma_multiply(0.5)
    };
    glyph(painter, rect.shrink(5.0), g, col);
    resp.on_hover_text(tip)
}

/// A glyph inline with text (lists, menus).
fn glyph_label(ui: &mut egui::Ui, g: Glyph) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(18.0, 16.0), egui::Sense::hover());
    glyph(ui.painter(), rect.shrink(1.0), g, ui.visuals().text_color());
}

fn shape_glyph(s: &SectionShape) -> Glyph {
    match s {
        SectionShape::Cylinder { .. } => Glyph::Cylinder,
        SectionShape::Cone { .. } => Glyph::Cone,
        SectionShape::Polygon { .. } => Glyph::Polygon,
    }
}

/// A new section of kind `g` next to diameter `d`.
fn new_section(g: Glyph, d: f64) -> Section {
    match g {
        Glyph::Cone => Section::cone(d, (d * 0.8).round(), 30.0),
        Glyph::Polygon => Section::polygon(6, d, 20.0),
        _ => Section::cylinder(d, 40.0),
    }
}

/// Inserts a section of kind `g` at index `at` (0 ..= n).
fn insert_at(p: &mut ShaftPanel, g: Glyph, at: usize) {
    let at = at.min(p.design.sections.len());
    let d = if at > 0 {
        p.design.sections.get(at - 1).map(|s| s.diameters().1)
    } else {
        p.design.sections.first().map(|s| s.diameters().0)
    }
    .unwrap_or(40.0);
    p.design.sections.insert(at, new_section(g, d));
    p.selected = at;
    p.edit = None;
}

/// Moves section `k` to the boundary `b` (0 ..= n).
fn move_to(p: &mut ShaftPanel, k: usize, b: usize) {
    if b == k || b == k + 1 || k >= p.design.sections.len() {
        return;
    }
    let s = p.design.sections.remove(k);
    let at = if b > k { b - 1 } else { b };
    p.design.sections.insert(at, s);
    p.selected = at;
    p.edit = None;
}

/// Turns the shaft end for end: the sections in reverse, each one's ends swapped.
fn flip(p: &mut ShaftPanel) {
    p.design.sections.reverse();
    for s in &mut p.design.sections {
        std::mem::swap(&mut s.start, &mut s.end);
        if let SectionShape::Cone { d1, d2 } = &mut s.shape {
            std::mem::swap(d1, d2);
        }
        let l = s.length;
        for g in &mut s.grooves {
            g.at = l - g.at - g.width;
        }
    }
    std::mem::swap(&mut p.design.bore_left, &mut p.design.bore_right);
    let n = p.design.sections.len();
    p.selected = n.saturating_sub(1).saturating_sub(p.selected);
    p.edit = None;
}

/// Puts a feature on section `k`: an edge feature at its left or right end,
/// or a section feature.
fn add_feature(p: &mut ShaftPanel, k: usize, g: Glyph, right: bool) {
    let Some(s) = p.design.sections.get_mut(k) else {
        return;
    };
    let d0 = s.diameters().0;
    let len = s.length;
    let cylinder = matches!(s.shape, SectionShape::Cylinder { .. });
    match g {
        Glyph::Chamfer => {
            let e = EdgeFeature::Chamfer((d0 / 30.0).clamp(0.5, 3.0).round().max(0.5));
            if right { s.end = e } else { s.start = e }
        }
        Glyph::Fillet => {
            let e = EdgeFeature::Fillet((d0 / 40.0).clamp(0.4, 2.5));
            if right { s.end = e } else { s.start = e }
        }
        Glyph::Keyway if cylinder => s.keyway = Some((len * 0.8).floor()),
        Glyph::Thread if cylinder => s.thread = true,
        Glyph::RingGroove if cylinder => s.grooves.push(RingGroove {
            at: len / 2.0,
            width: (d0 / 25.0).clamp(0.8, 4.2),
            depth: (d0 / 40.0).clamp(0.3, 3.0),
            relief: false,
        }),
        Glyph::Relief if cylinder => {
            let w = (d0 / 12.0).clamp(1.0, 4.0);
            s.grooves.push(RingGroove {
                at: if right { len - w } else { 0.0 },
                width: w,
                depth: (d0 / 100.0).clamp(0.2, 0.5),
                relief: true,
            });
        }
        _ => {}
    }
    clear_overlaps(&mut p.design, k, g, right);
    p.selected = k;
}

/// A relief takes the place of the chamfer or fillet at its end (and an edge
/// feature added later takes the relief's): the two would run the outline
/// back on itself.
fn clear_overlaps(d: &mut ShaftDesign, k: usize, g: Glyph, right: bool) {
    let n = d.sections.len();
    if k >= n {
        return;
    }
    if g == Glyph::Relief {
        // This section's edge first, then the neighbour's that reaches in.
        for _ in 0..2 {
            if d.edge_extent(k, right) <= 0.0 {
                break;
            }
            let s = &mut d.sections[k];
            let own = if right { &mut s.end } else { &mut s.start };
            if *own != EdgeFeature::None {
                *own = EdgeFeature::None;
            } else if right && k + 1 < n {
                d.sections[k + 1].start = EdgeFeature::None;
            } else if !right && k > 0 {
                d.sections[k - 1].end = EdgeFeature::None;
            }
        }
    } else if matches!(g, Glyph::Chamfer | Glyph::Fillet) {
        // Every section next to the corner: reliefs the new edge reaches.
        let near = [k.checked_sub(1), Some(k), (k + 1 < n).then_some(k + 1)];
        for i in near.into_iter().flatten() {
            let (a0, a1) = (d.edge_extent(i, false), d.edge_extent(i, true));
            let len = d.sections[i].length;
            d.sections[i].grooves.retain(|gr| {
                !gr.relief || (gr.at >= a0 - 1e-9 && gr.at + gr.width <= len - a1 + 1e-9)
            });
        }
    }
}

fn design_tab(p: &mut ShaftPanel, ui: &mut egui::Ui) {
    toolbar(p, ui);
    ui.add_space(2.0);
    profile(p, ui);
    if let Err(e) = p.design.validate() {
        ui.colored_label(super::theme::tokens().error, format!("Shaft: {e}"));
    }
    ui.add_space(4.0);
    ui.columns(2, |c| {
        section_list(p, &mut c[0]);
        section_editor(p, &mut c[1]);
    });
    edge_menu(p, ui.ctx());
}

/// The icons above the picture.
fn toolbar(p: &mut ShaftPanel, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        let n = p.design.sections.len();
        let k = p.selected;
        for (g, name) in [
            (Glyph::Cylinder, "Insert Cylinder"),
            (Glyph::Cone, "Insert Cone"),
            (Glyph::Polygon, "Insert Polygon"),
        ] {
            let tip = format!(
                "{name}: click where it goes in the picture (the marker shows the place); \
                 click the icon again to put it after the selected section"
            );
            if icon_button(ui, g, &tip, true, p.placing == Some(g)).clicked() {
                if p.placing == Some(g) {
                    insert_at(p, g, k + 1);
                    p.placing = None;
                } else {
                    p.placing = Some(g);
                }
            }
        }
        ui.separator();
        if icon_button(
            ui,
            Glyph::MoveLeft,
            "Move Left (or drag the section in the picture)",
            k > 0,
            false,
        )
        .clicked()
        {
            move_to(p, k, k - 1);
        }
        if icon_button(
            ui,
            Glyph::MoveRight,
            "Move Right (or drag the section in the picture)",
            k + 1 < n,
            false,
        )
        .clicked()
        {
            move_to(p, k, k + 2);
        }
        if icon_button(
            ui,
            Glyph::Delete,
            "Delete the selected section",
            n > 1,
            false,
        )
        .clicked()
        {
            p.design.sections.remove(k);
            p.selected = k.min(p.design.sections.len().saturating_sub(1));
            p.edit = None;
        }
        if icon_button(
            ui,
            Glyph::Flip,
            "Flip: turn the shaft end for end",
            n > 1,
            false,
        )
        .clicked()
        {
            flip(p);
        }
        ui.separator();
        let cylinder = p
            .design
            .sections
            .get(k)
            .is_some_and(|s| matches!(s.shape, SectionShape::Cylinder { .. }));
        for (g, name, edges) in [
            (
                Glyph::Chamfer,
                "Chamfer: left or right edge of the selected section",
                true,
            ),
            (
                Glyph::Fillet,
                "Fillet: left or right edge of the selected section",
                true,
            ),
            (
                Glyph::RingGroove,
                "Retaining Ring Groove on the selected section",
                false,
            ),
            (
                Glyph::Relief,
                "Relief Groove: left or right end of the selected section",
                true,
            ),
            (
                Glyph::Keyway,
                "Keyway Groove (DIN 6885) on the selected section",
                false,
            ),
            (
                Glyph::Thread,
                "Thread (ISO metric) on the selected section",
                false,
            ),
        ] {
            let ok = cylinder || matches!(g, Glyph::Chamfer | Glyph::Fillet);
            let r = icon_button(ui, g, name, ok, p.edge_menu.is_some_and(|m| m.0 == g));
            if r.clicked() {
                if edges {
                    p.edge_menu = Some((g, r.rect.left_bottom()));
                } else {
                    add_feature(p, k, g, false);
                }
            }
        }
        icon_button(
            ui,
            Glyph::Splines,
            "Splines (ISO 14 / DIN 5480): spec 6.2-design-calculators §3.1, the tables come later",
            false,
            false,
        );
        ui.separator();
        for (g, end, name) in [
            (Glyph::BoreLeft, false, "Bore on the Left"),
            (Glyph::BoreRight, true, "Bore on the Right"),
        ] {
            let on = if end {
                p.design.bore_right.is_some()
            } else {
                p.design.bore_left.is_some()
            };
            let tip =
                format!("{name}: a cylindrical hole into that end (click again to remove it)");
            if icon_button(ui, g, &tip, n > 0, on).clicked() {
                toggle_bore(&mut p.design, end);
            }
        }
        ui.add_enabled(false, egui::Button::new("Split").small())
            .on_disabled_hover_text("Spec 6.2-design-calculators §3.1: comes later");
    });
}

/// The Left / Right choice of an edge feature from the toolbar.
fn edge_menu(p: &mut ShaftPanel, ctx: &egui::Context) {
    let Some((g, at)) = p.edge_menu else {
        return;
    };
    let mut pick = None;
    let area = egui::Area::new(egui::Id::new("shaft_edge_menu"))
        .order(egui::Order::Foreground)
        .fixed_pos(at)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                for (right, text) in [(false, "Left Edge"), (true, "Right Edge")] {
                    ui.horizontal(|ui| {
                        glyph_label(ui, g);
                        if ui.button(text).clicked() {
                            pick = Some(right);
                        }
                    });
                }
            });
        });
    if let Some(right) = pick {
        add_feature(p, p.selected, g, right);
        p.edge_menu = None;
    } else if ctx.input(|i| i.key_pressed(egui::Key::Escape))
        || (ctx.input(|i| i.pointer.any_pressed())
            && ctx
                .pointer_interact_pos()
                .is_some_and(|q| !area.response.rect.contains(q) && q.y > at.y + 2.0))
    {
        p.edge_menu = None;
    }
}

/// Adds a bore at one end (a third of the end section's diameter, half its
/// length deep), or takes it away.
fn toggle_bore(d: &mut ShaftDesign, end: bool) {
    let slot = if end {
        &mut d.bore_right
    } else {
        &mut d.bore_left
    };
    if slot.take().is_some() {
        return;
    }
    let s = if end {
        d.sections.last()
    } else {
        d.sections.first()
    };
    if let Some(s) = s {
        let (d0, d1) = s.diameters();
        let dia = (if end { d1 } else { d0 } / 3.0).round().max(1.0);
        *slot = Some(Bore {
            d: dia,
            depth: (s.length / 2.0).round().max(1.0),
        });
    }
}

/// The bores' rows under the section list.
fn bore_rows(p: &mut ShaftPanel, ui: &mut egui::Ui) {
    for (end, g, name) in [
        (false, Glyph::BoreLeft, "Bore on the Left"),
        (true, Glyph::BoreRight, "Bore on the Right"),
    ] {
        let b = if end {
            &mut p.design.bore_right
        } else {
            &mut p.design.bore_left
        };
        let Some(b) = b.as_mut() else {
            continue;
        };
        let mut gone = false;
        ui.horizontal_wrapped(|ui| {
            glyph_label(ui, g);
            ui.label(name);
            ui.label("D");
            mm(ui, &mut b.d);
            ui.label("depth");
            mm(ui, &mut b.depth);
            gone = ui.small_button("Delete").clicked();
        });
        if gone {
            if end {
                p.design.bore_right = None;
            } else {
                p.design.bore_left = None;
            }
        }
    }
}

fn section_list(p: &mut ShaftPanel, ui: &mut egui::Ui) {
    ui.label(RichText::new("Sections").strong());
    let mut z = 0.0;
    for (k, s) in p.design.sections.iter().enumerate() {
        ui.horizontal(|ui| {
            glyph_label(ui, shape_glyph(&s.shape));
            if ui
                .selectable_label(p.selected == k, format!("{}. {}", k + 1, s.label()))
                .on_hover_text(format!("from z = {} mm", num(z)))
                .clicked()
            {
                p.selected = k;
            }
            for e in [s.start, s.end] {
                match e {
                    EdgeFeature::Chamfer(_) => glyph_label(ui, Glyph::Chamfer),
                    EdgeFeature::Fillet(_) => glyph_label(ui, Glyph::Fillet),
                    EdgeFeature::None => {}
                }
            }
            if s.keyway.is_some() {
                glyph_label(ui, Glyph::Keyway);
            }
            if s.thread {
                glyph_label(ui, Glyph::Thread);
            }
            for g in &s.grooves {
                glyph_label(
                    ui,
                    if g.relief {
                        Glyph::Relief
                    } else {
                        Glyph::RingGroove
                    },
                );
            }
        });
        z += s.length;
    }
    bore_rows(p, ui);
}

/// A length field in mm.
fn mm(ui: &mut egui::Ui, v: &mut f64) -> bool {
    ui.add(ExprValue::new(v).suffix(" mm")).changed()
}

fn section_editor(p: &mut ShaftPanel, ui: &mut egui::Ui) {
    let sel = p.selected;
    let Some(s) = p.design.sections.get_mut(sel) else {
        return;
    };
    ui.label(RichText::new(format!("Section {}: {}", sel + 1, s.label())).strong());
    egui::Grid::new("shaft_section_grid")
        .num_columns(2)
        .show(ui, |ui| {
            match &mut s.shape {
                SectionShape::Cylinder { d } => {
                    ui.label("Main Diameter D");
                    mm(ui, d);
                    ui.end_row();
                }
                SectionShape::Cone { d1, d2 } => {
                    ui.label("First Diameter D1");
                    mm(ui, d1);
                    ui.end_row();
                    ui.label("Second Diameter D2");
                    mm(ui, d2);
                    ui.end_row();
                }
                SectionShape::Polygon { sides, d } => {
                    ui.label("Number of Edges N");
                    let mut n = f64::from(*sides);
                    if ui.add(ExprValue::new(&mut n).range(3.0..=64.0)).changed() {
                        *sides = n.round() as u32;
                    }
                    ui.end_row();
                    ui.label("Diameter Circumscribed");
                    mm(ui, d);
                    ui.end_row();
                }
            }
            ui.label("Length L");
            mm(ui, &mut s.length);
            ui.end_row();
            edge_row(ui, "Left Edge", &mut s.start, ("shaft_start", sel));
            edge_row(ui, "Right Edge", &mut s.end, ("shaft_end", sel));
        });
    ui.add_space(4.0);
    ui.label(RichText::new("Section Features").strong());
    let cylinder = matches!(s.shape, SectionShape::Cylinder { .. });
    let d0 = s.diameters().0;
    ui.horizontal_wrapped(|ui| {
        let mut on = s.keyway.is_some();
        if ui
            .add_enabled(cylinder, egui::Checkbox::new(&mut on, "Keyway Groove"))
            .on_hover_text(
                "A DIN 6885 form A groove for a parallel key, centred on the section and cut \
                 into the model; the calculation takes its notch factor (RP-1123 Table V)",
            )
            .changed()
        {
            s.keyway = on.then_some((s.length * 0.8).floor());
        }
        if let Some(l) = s.keyway.as_mut() {
            ui.label("length");
            mm(ui, l);
            match key::size_for(d0) {
                Ok(k) => ui.label(
                    RichText::new(format!("DIN 6885 A {}×{}, t1 {} mm", k.b, k.h, k.t1)).weak(),
                ),
                Err(e) => ui.colored_label(super::theme::tokens().error, e),
            };
        }
    });
    ui.horizontal(|ui| {
        ui.add_enabled(cylinder, egui::Checkbox::new(&mut s.thread, "Thread"))
            .on_hover_text("A cosmetic ISO metric coarse thread over the section");
        if s.thread {
            ui.label(RichText::new(format!("ISO metric M{}", num(d0))).weak());
        }
    });
    ui.add_enabled(
        false,
        egui::Checkbox::new(&mut false, "Splines (ISO 14 / DIN 5480)"),
    )
    .on_disabled_hover_text("Spec 6.2-design-calculators §3.1: the spline tables come later");
    let mut gone = None;
    for (g, gr) in s.grooves.iter_mut().enumerate() {
        ui.horizontal_wrapped(|ui| {
            ui.label(if gr.relief {
                "Relief Groove"
            } else {
                "Retaining Ring Groove"
            });
            ui.label("at");
            mm(ui, &mut gr.at);
            ui.label("width m");
            mm(ui, &mut gr.width);
            ui.label("depth t");
            mm(ui, &mut gr.depth);
            if ui.small_button("Delete").clicked() {
                gone = Some(g);
            }
        });
    }
    if let Some(g) = gone {
        s.grooves.remove(g);
    }
    ui.horizontal_wrapped(|ui| {
        let len = s.length;
        if ui
            .add_enabled(cylinder, egui::Button::new("Add Retaining Ring Groove"))
            .on_disabled_hover_text("Grooves go into cylinders")
            .clicked()
        {
            s.grooves.push(RingGroove {
                at: len / 2.0,
                width: (d0 / 25.0).clamp(0.8, 4.2),
                depth: (d0 / 40.0).clamp(0.3, 3.0),
                relief: false,
            });
        }
        for (end, text) in [(false, "Add Relief at Left"), (true, "Add Relief at Right")] {
            if ui
                .add_enabled(cylinder, egui::Button::new(text))
                .on_hover_text("An undercut at the shoulder for a grinding wheel or a thread end")
                .on_disabled_hover_text("Grooves go into cylinders")
                .clicked()
            {
                let w = (d0 / 12.0).clamp(1.0, 4.0);
                s.grooves.push(RingGroove {
                    at: if end { len - w } else { 0.0 },
                    width: w,
                    depth: (d0 / 100.0).clamp(0.2, 0.5),
                    relief: true,
                });
            }
        }
    });
}

fn edge_row(ui: &mut egui::Ui, label: &str, e: &mut EdgeFeature, id: (&str, usize)) {
    ui.label(label);
    ui.horizontal(|ui| {
        let names = ["None", "Chamfer", "Fillet"];
        let mut k = match e {
            EdgeFeature::None => 0,
            EdgeFeature::Chamfer(_) => 1,
            EdgeFeature::Fillet(_) => 2,
        };
        let mut size = match e {
            EdgeFeature::Chamfer(v) | EdgeFeature::Fillet(v) => *v,
            EdgeFeature::None => 1.0,
        };
        egui::ComboBox::from_id_salt(id)
            .width(80.0)
            .selected_text(names[k])
            .show_ui(ui, |ui| {
                for (i, n) in names.iter().enumerate() {
                    ui.selectable_value(&mut k, i, *n);
                }
            });
        if k > 0 {
            mm(ui, &mut size);
        }
        *e = match k {
            1 => EdgeFeature::Chamfer(size),
            2 => EdgeFeature::Fillet(size),
            _ => EdgeFeature::None,
        };
    });
    ui.end_row();
}

/// The profile drawn to scale with its dimensions; a click on a section selects
/// it, a click on a dimension opens its value in place.
fn profile(p: &mut ShaftPanel, ui: &mut egui::Ui) {
    let t = super::theme::tokens();
    let w = ui.available_width().max(300.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 250.0), egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, t.field);
    let len = p.design.length().max(1e-6);
    let dmax = p
        .design
        .sections
        .iter()
        .map(|s| match s.shape {
            SectionShape::Polygon { d, .. } => d,
            _ => {
                let (a, b) = s.diameters();
                a.max(b)
            }
        })
        .fold(1.0, f64::max);
    // Room for the diameters above and the length dimensions below.
    let scale =
        ((f64::from(rect.width()) - 60.0) / len).min((f64::from(rect.height()) - 110.0) / dmax);
    let x0 = f64::from(rect.left()) + (f64::from(rect.width()) - len * scale) / 2.0;
    let cy = f64::from(rect.top()) + 34.0 + dmax * scale / 2.0;
    let pt = |z: f64, r: f64| Pos2::new((x0 + z * scale) as f32, (cy - r * scale) as f32);
    let (ink, weak) = (t.text, t.text_weak);
    // Centre line, dashed.
    let mut x = rect.left() + 6.0;
    while x < rect.right() - 6.0 {
        painter.line_segment(
            [Pos2::new(x, cy as f32), Pos2::new(x + 10.0, cy as f32)],
            Stroke::new(1.0, weak),
        );
        x += 16.0;
    }
    let hover = resp.hover_pos();
    let mut clicked_section = None;
    let mut dims: Vec<(Rect, Dim)> = Vec::new();
    let mut bodies: Vec<Rect> = Vec::new();
    let starts = p.design.starts();
    let bottom = (cy + dmax * scale / 2.0) as f32;
    for (k, s) in p.design.sections.iter().enumerate() {
        let z0 = starts[k];
        let z1 = z0 + s.length;
        let (d1, d2) = match s.shape {
            SectionShape::Polygon { d, .. } => (d, d),
            _ => s.diameters(),
        };
        let (r1, r2) = (d1 / 2.0, d2 / 2.0);
        let prev = (k > 0).then(|| p.design.sections[k - 1].diameters().1 / 2.0);
        let next = p.design.sections.get(k + 1).map(|q| q.diameters().0 / 2.0);
        // A chamfer shows where this section is the larger one (or at a shaft end).
        let start_c = match s.start {
            EdgeFeature::Chamfer(c) if prev.is_none_or(|r| r < r1) => c,
            _ => 0.0,
        };
        let end_c = match s.end {
            EdgeFeature::Chamfer(c) if next.is_none_or(|r| r < r2) => c,
            _ => 0.0,
        };
        let mut top: Vec<(f64, f64)> = vec![(z0, r1 - start_c), (z0 + start_c, r1)];
        if matches!(s.shape, SectionShape::Cylinder { .. }) {
            for g in &s.grooves {
                let (a, b) = (z0 + g.at, z0 + g.at + g.width);
                top.extend([(a, r1), (a, r1 - g.depth), (b, r1 - g.depth), (b, r1)]);
            }
        }
        top.extend([(z1 - end_c, r2), (z1, r2 - end_c)]);
        let mut outline: Vec<Pos2> = top.iter().map(|&(z, r)| pt(z, r)).collect();
        outline.extend(top.iter().rev().map(|&(z, r)| pt(z, -r)));
        let body = Rect::from_two_pos(pt(z0, r1.max(r2)), pt(z1, -r1.max(r2)));
        bodies.push(body);
        let hot = hover.is_some_and(|h| body.contains(h));
        let fill = if p.selected == k {
            t.selection
        } else if hot {
            t.control_hover
        } else {
            t.control
        };
        // Filled as quads between the top and bottom points (a groove makes
        // the outline concave).
        for w2 in top.windows(2) {
            let (a, b) = (w2[0], w2[1]);
            let quad = vec![pt(a.0, a.1), pt(b.0, b.1), pt(b.0, -b.1), pt(a.0, -a.1)];
            painter.add(egui::Shape::convex_polygon(quad, fill, Stroke::NONE));
        }
        painter.add(egui::Shape::closed_line(outline, Stroke::new(1.5, ink)));
        if let SectionShape::Polygon { sides, d } = s.shape {
            // The flats seen from the side: the inscribed circle's lines.
            let rin = d / 2.0 * (std::f64::consts::PI / f64::from(sides)).cos();
            for r in [rin, -rin] {
                painter.line_segment([pt(z0, r), pt(z1, r)], Stroke::new(1.0, weak));
            }
        }
        if let Some(l) = s.keyway {
            let kz0 = z0 + (s.length - l) / 2.0;
            let depth = key::size_for(d1).map_or(d1 * 0.12, |k| k.t1);
            let a = pt(kz0, r1);
            let b = pt(kz0 + l, r1 - depth);
            painter.rect_stroke(
                Rect::from_two_pos(a, b),
                2.0,
                Stroke::new(1.2, t.accent),
                egui::StrokeKind::Middle,
            );
            let r = dim_text(
                &painter,
                Pos2::new((a.x + b.x) / 2.0, b.y + 9.0),
                &format!("key {}", num(l)),
                t.accent,
            );
            dims.push((r, Dim::Keyway(k)));
        }
        if resp.clicked() && hot {
            clicked_section = Some(k);
        }
        // Diameter above the section.
        let xm = (pt(z0, 0.0).x + pt(z1, 0.0).x) / 2.0;
        let label = match s.shape {
            SectionShape::Polygon { sides, .. } => format!("{sides}× D{}", num(d1)),
            _ => format!("Ø{}", num(d1)),
        };
        let r = dim_text(
            &painter,
            Pos2::new(xm, pt(0.0, r1.max(r2)).y - 10.0),
            &label,
            ink,
        );
        dims.push((r, Dim::Diameter(k)));
        if let SectionShape::Cone { .. } = s.shape {
            let r = dim_text(
                &painter,
                Pos2::new(pt(z1, 0.0).x - 16.0, pt(0.0, r2).y - 10.0),
                &format!("Ø{}", num(d2)),
                ink,
            );
            dims.push((r, Dim::Diameter2(k)));
        }
        // Chamfers and fillets: their size beside the corner, below the shaft.
        for (end, e, zc, rc) in [(false, s.start, z0, r1), (true, s.end, z1, r2)] {
            let text = match e {
                EdgeFeature::Chamfer(c) => format!("{}×45°", num(c)),
                EdgeFeature::Fillet(f) => format!("R{}", num(f)),
                EdgeFeature::None => continue,
            };
            let x = pt(zc, 0.0).x + if end { -18.0 } else { 18.0 };
            let r = dim_text(&painter, Pos2::new(x, pt(0.0, -rc).y + 9.0), &text, weak);
            dims.push((r, Dim::Edge(k, end)));
        }
        // Grooves: their width below the groove.
        for (g, gr) in s.grooves.iter().enumerate() {
            let x = pt(z0 + gr.at + gr.width / 2.0, 0.0).x;
            let r = dim_text(
                &painter,
                Pos2::new(x, pt(0.0, -r1).y + 21.0),
                &format!("{}{}", if gr.relief { "rel " } else { "m " }, num(gr.width)),
                weak,
            );
            dims.push((r, Dim::GrooveWidth(k, g)));
        }
        // Length: a dimension line below the shaft.
        let y = bottom + 34.0;
        let (a, b) = (pt(z0, 0.0).x, pt(z1, 0.0).x);
        let col = if p.selected == k { t.accent } else { ink };
        painter.line_segment([Pos2::new(a, y), Pos2::new(b, y)], Stroke::new(1.0, col));
        for x in [a, b] {
            painter.line_segment(
                [Pos2::new(x, y - 5.0), Pos2::new(x, y + 5.0)],
                Stroke::new(1.0, col),
            );
        }
        let r = dim_text(
            &painter,
            Pos2::new((a + b) / 2.0, y - 8.0),
            &num(s.length),
            col,
        );
        dims.push((r, Dim::Length(k)));
    }
    // Bores: dashed lines inside, their diameter and depth clickable.
    for (end, b) in p.design.bores() {
        let (za, zb) = if end {
            (len - b.depth, len)
        } else {
            (0.0, b.depth)
        };
        let r = b.d / 2.0;
        let dash = Stroke::new(1.2, t.accent);
        for (q0, q1) in [
            (pt(za, r), pt(zb, r)),
            (pt(za, -r), pt(zb, -r)),
            (
                pt(if end { za } else { zb }, r),
                pt(if end { za } else { zb }, -r),
            ),
        ] {
            painter.add(egui::Shape::dashed_line(&[q0, q1], dash, 5.0, 3.0));
        }
        let x = (pt(za, 0.0).x + pt(zb, 0.0).x) / 2.0;
        let rd = dim_text(
            &painter,
            Pos2::new(x, pt(0.0, r).y - 8.0),
            &format!("Ø{}", num(b.d)),
            t.accent,
        );
        dims.push((rd, Dim::BoreD(end)));
        let rl = dim_text(
            &painter,
            Pos2::new(x, pt(0.0, -r).y + 8.0),
            &format!("bore {}", num(b.depth)),
            t.accent,
        );
        dims.push((rl, Dim::BoreDepth(end)));
    }
    // Overall length.
    let y = bottom + 60.0;
    let (a, b) = (pt(0.0, 0.0).x, pt(len, 0.0).x);
    painter.line_segment([Pos2::new(a, y), Pos2::new(b, y)], Stroke::new(1.0, weak));
    dim_text(
        &painter,
        Pos2::new((a + b) / 2.0, y + 9.0),
        &format!("L = {} mm", num(len)),
        weak,
    );
    // The boundaries between sections (and the two ends), where a new
    // section or a dragged one goes.
    let mut bx: Vec<f32> = starts.iter().map(|&z| pt(z, 0.0).x).collect();
    bx.push(pt(len, 0.0).x);
    let nearest = |x: f32| -> usize {
        bx.iter()
            .enumerate()
            .min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs()))
            .map_or(0, |(i, _)| i)
    };
    let top_y = rect.top() + 6.0;
    let bot_y = bottom + 20.0;
    let marker = |b: usize, col: Color32, label: &str| {
        let x = bx[b.min(bx.len() - 1)];
        painter.line_segment(
            [Pos2::new(x, top_y + 8.0), Pos2::new(x, bot_y)],
            Stroke::new(2.0, col),
        );
        painter.add(egui::Shape::convex_polygon(
            vec![
                Pos2::new(x - 6.0, top_y),
                Pos2::new(x + 6.0, top_y),
                Pos2::new(x, top_y + 9.0),
            ],
            col,
            Stroke::NONE,
        ));
        if !label.is_empty() {
            painter.text(
                Pos2::new(x + 8.0, top_y + 2.0),
                egui::Align2::LEFT_TOP,
                label,
                egui::FontId::proportional(11.0),
                col,
            );
        }
    };
    let section_at = |q: Pos2| bodies.iter().position(|b| b.contains(q));
    // Drag a section along the shaft to reorder it.
    if resp.drag_started()
        && p.placing.is_none()
        && let Some(o) = ui.input(|i| i.pointer.press_origin())
        && !dims.iter().any(|(r, _)| r.expand(2.0).contains(o))
    {
        p.drag = section_at(o);
    }
    if let Some(k) = p.drag {
        let at = resp.interact_pointer_pos().or(hover);
        if let Some(h) = at {
            let b = nearest(h.x);
            if b != k && b != k + 1 {
                marker(b, t.accent, "move here");
            }
        }
        if let Some(body) = bodies.get(k) {
            painter.rect_stroke(
                *body,
                2.0,
                Stroke::new(2.0, t.accent),
                egui::StrokeKind::Outside,
            );
        }
        if resp.drag_stopped() {
            if let Some(h) = at {
                move_to(p, k, nearest(h.x));
            }
            p.drag = None;
        }
    }
    // Insert: the marker follows the cursor; a click puts the section there.
    let mut inserted = false;
    if let Some(g) = p.placing {
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            p.placing = None;
        } else if let Some(h) = hover {
            let b = nearest(h.x);
            let what = match g {
                Glyph::Cone => "insert cone here",
                Glyph::Polygon => "insert polygon here",
                _ => "insert cylinder here",
            };
            marker(b, t.accent, what);
            if resp.clicked() {
                insert_at(p, g, b);
                p.placing = None;
                inserted = true;
            }
        } else {
            marker(
                p.selected + 1,
                t.accent,
                "move into the picture and click the place",
            );
        }
    } else if p.drag.is_none() {
        // The insertion point is always shown: after the selected section.
        marker(p.selected + 1, t.text_weak, "");
    }
    // A dimension under the cursor is lit; a click opens its box.
    let mut on_dim = false;
    if p.placing.is_none() && p.drag.is_none() && !inserted {
        for (r, dim) in &dims {
            if hover.is_some_and(|h| r.expand(2.0).contains(h)) {
                on_dim = true;
                painter.rect_stroke(
                    r.expand(2.0),
                    2.0,
                    Stroke::new(1.0, t.accent),
                    egui::StrokeKind::Outside,
                );
                if resp.clicked() {
                    p.edit = Some(DimEdit {
                        dim: *dim,
                        at: r.left_bottom() + Vec2::new(-8.0, 3.0),
                        fresh: 2,
                    });
                    p.selected = dim_section(dim, p.design.sections.len());
                }
            }
        }
    }
    if on_dim {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    } else if p.placing.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
    } else if p.drag.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if !inserted && let Some(k) = clicked_section {
        p.selected = k;
    }
    // The right-click menu of the section under the cursor.
    if resp.secondary_clicked()
        && let Some(k) = hover.and_then(section_at)
    {
        p.selected = k;
    }
    let n = p.design.sections.len();
    let resp = resp.on_hover_text(
        "Click a section to select it, drag it to move it, click a dimension to change it; \
         right click for its menu",
    );
    resp.context_menu(|ui| {
        let k = p.selected;
        for (before, text) in [(true, "Insert Before"), (false, "Insert After")] {
            ui.menu_button(text, |ui| {
                for (g, name) in [
                    (Glyph::Cylinder, "Cylinder"),
                    (Glyph::Cone, "Cone"),
                    (Glyph::Polygon, "Polygon"),
                ] {
                    ui.horizontal(|ui| {
                        glyph_label(ui, g);
                        if ui.button(name).clicked() {
                            insert_at(p, g, if before { k } else { k + 1 });
                            ui.close();
                        }
                    });
                }
            });
        }
        ui.separator();
        ui.horizontal(|ui| {
            glyph_label(ui, Glyph::MoveLeft);
            if ui
                .add_enabled(k > 0, egui::Button::new("Move Left"))
                .clicked()
            {
                move_to(p, k, k - 1);
                ui.close();
            }
        });
        ui.horizontal(|ui| {
            glyph_label(ui, Glyph::MoveRight);
            if ui
                .add_enabled(k + 1 < n, egui::Button::new("Move Right"))
                .clicked()
            {
                move_to(p, k, k + 2);
                ui.close();
            }
        });
        ui.horizontal(|ui| {
            glyph_label(ui, Glyph::Delete);
            if ui.add_enabled(n > 1, egui::Button::new("Delete")).clicked() {
                p.design.sections.remove(k);
                p.selected = k.min(p.design.sections.len().saturating_sub(1));
                p.edit = None;
                ui.close();
            }
        });
        ui.separator();
        ui.menu_button("Add Feature", |ui| {
            for (g, name, right) in [
                (Glyph::Chamfer, "Chamfer at Left", false),
                (Glyph::Chamfer, "Chamfer at Right", true),
                (Glyph::Fillet, "Fillet at Left", false),
                (Glyph::Fillet, "Fillet at Right", true),
                (Glyph::RingGroove, "Retaining Ring Groove", false),
                (Glyph::Relief, "Relief at Left", false),
                (Glyph::Relief, "Relief at Right", true),
                (Glyph::Keyway, "Keyway Groove", false),
                (Glyph::Thread, "Thread", false),
            ] {
                ui.horizontal(|ui| {
                    glyph_label(ui, g);
                    if ui.button(name).clicked() {
                        add_feature(p, k, g, right);
                        ui.close();
                    }
                });
            }
            ui.horizontal(|ui| {
                glyph_label(ui, Glyph::Splines);
                ui.add_enabled(false, egui::Button::new("Splines"))
                    .on_disabled_hover_text("The spline tables come later");
            });
        });
    });
}

fn dim_section(d: &Dim, n: usize) -> usize {
    match *d {
        Dim::Length(k)
        | Dim::Diameter(k)
        | Dim::Diameter2(k)
        | Dim::Edge(k, _)
        | Dim::Keyway(k)
        | Dim::GrooveWidth(k, _) => k,
        Dim::BoreD(end) | Dim::BoreDepth(end) => {
            if end {
                n.saturating_sub(1)
            } else {
                0
            }
        }
    }
}

/// A dimension's text centred at `at`; its rectangle.
fn dim_text(painter: &egui::Painter, at: Pos2, text: &str, color: Color32) -> Rect {
    painter.text(
        at,
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::proportional(12.0),
        color,
    )
}

/// The value a dimension stands for.
fn dim_value(design: &mut ShaftDesign, dim: Dim) -> Option<&mut f64> {
    match dim {
        Dim::BoreD(end) | Dim::BoreDepth(end) => {
            let b = if end {
                design.bore_right.as_mut()
            } else {
                design.bore_left.as_mut()
            }?;
            return Some(if matches!(dim, Dim::BoreD(_)) {
                &mut b.d
            } else {
                &mut b.depth
            });
        }
        _ => {}
    }
    let n = design.sections.len();
    let s = design.sections.get_mut(dim_section(&dim, n))?;
    match dim {
        Dim::Length(_) => Some(&mut s.length),
        Dim::Diameter(_) => match &mut s.shape {
            SectionShape::Cylinder { d } | SectionShape::Polygon { d, .. } => Some(d),
            SectionShape::Cone { d1, .. } => Some(d1),
        },
        Dim::Diameter2(_) => match &mut s.shape {
            SectionShape::Cone { d2, .. } => Some(d2),
            _ => None,
        },
        Dim::Edge(_, end) => match if end { &mut s.end } else { &mut s.start } {
            EdgeFeature::Chamfer(v) | EdgeFeature::Fillet(v) => Some(v),
            EdgeFeature::None => None,
        },
        Dim::Keyway(_) => s.keyway.as_mut(),
        Dim::GrooveWidth(_, g) => s.grooves.get_mut(g).map(|g| &mut g.width),
        Dim::BoreD(_) | Dim::BoreDepth(_) => None,
    }
}

/// The in-place box of the dimension being edited: the expression field with
/// units; Enter applies, Esc or a click elsewhere closes.
fn dim_edit_box(p: &mut ShaftPanel, ctx: &egui::Context) {
    let Some(mut e) = p.edit else {
        return;
    };
    let fresh = e.fresh;
    if fresh > 0 {
        ctx.request_repaint();
    }
    let Some(v) = dim_value(&mut p.design, e.dim) else {
        p.edit = None;
        return;
    };
    let mut done = false;
    let area = egui::Area::new(egui::Id::new("shaft_dim_edit"))
        .order(egui::Order::Foreground)
        .fixed_pos(e.at)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                let id = ui.next_auto_id();
                let n = crate::part::params::format_value(*v, crate::part::expr::Unit::Mm)
                    .chars()
                    .count();
                let r = ui.add(ExprValue::new(&mut *v).suffix(" mm"));
                if e.fresh == 1 {
                    // Focused with its text selected, so typing replaces it.
                    let te = id.with("te");
                    let mut st = egui::TextEdit::load_state(ui.ctx(), te).unwrap_or_default();
                    st.cursor.set_char_range(Some(egui::text::CCursorRange::two(
                        egui::text::CCursor::new(0),
                        egui::text::CCursor::new(n),
                    )));
                    st.store(ui.ctx(), te);
                    ui.memory_mut(|m| m.request_focus(te));
                }
                e.fresh = e.fresh.saturating_sub(1);
                if r.changed() {
                    done = true;
                }
            });
        });
    let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    let away = fresh == 0
        && ctx.input(|i| i.pointer.any_pressed())
        && ctx
            .pointer_interact_pos()
            .is_some_and(|q| !area.response.rect.contains(q));
    p.edit = (!(done || esc || away)).then_some(e);
}

// ---------------------------------------------------------- Calculation tab

/// A value field with its unit; the value is kept in `scale` × the unit (N·m
/// shown, N·mm kept: 1000).
fn qty(ui: &mut egui::Ui, v: &mut f64, unit: &'static str, scale: f64) -> bool {
    let mut shown = *v / scale;
    let changed = ui.add(ExprValue::new(&mut shown).unit(unit)).changed();
    if changed {
        *v = shown * scale;
    }
    changed
}

fn angle_field(ui: &mut egui::Ui, a: &mut f64) {
    let mut deg = a.to_degrees();
    if ui.add(ExprValue::new(&mut deg).suffix("°")).changed() {
        *a = deg.to_radians();
    }
}

fn calculation_tab(p: &mut ShaftPanel, ui: &mut egui::Ui) {
    ui.columns(2, |c| {
        calc_inputs(p, &mut c[0]);
        calc_results(p, &mut c[1]);
    });
}

fn calc_inputs(p: &mut ShaftPanel, ui: &mut egui::Ui) {
    let presets = Material::presets();
    ui.label(RichText::new("Material").strong());
    let name = presets
        .get(p.preset)
        .map_or("User material".to_owned(), |m| m.name.clone());
    egui::ComboBox::from_id_salt("shaft_material")
        .selected_text(name)
        .width(260.0)
        .show_ui(ui, |ui| {
            for (i, m) in presets.iter().enumerate() {
                if ui
                    .selectable_label(p.preset == i, m.name.as_str())
                    .clicked()
                {
                    p.preset = i;
                    p.loads.material = m.clone();
                }
            }
        });
    let m = &mut p.loads.material;
    let mut typed = false;
    egui::Grid::new("shaft_material_grid")
        .num_columns(2)
        .show(ui, |ui| {
            for (label, v) in [
                ("Ultimate Strength σu", &mut m.ultimate),
                ("Yield Strength σy", &mut m.yield_strength),
                ("Fatigue Limit σ'f", &mut m.fatigue_limit),
                ("Modulus of Elasticity E", &mut m.e),
                ("Modulus of Rigidity G", &mut m.g),
            ] {
                ui.label(label);
                typed |= qty(ui, v, "MPa", 1.0);
                ui.end_row();
            }
            ui.label("Density ρ");
            typed |= qty(ui, &mut m.density, "kg/m³", 1.0);
            ui.end_row();
        });
    typed |= ui
        .checkbox(&mut m.hardened, "Hardened above 200 BHN (keyway factors)")
        .changed();
    if typed {
        p.preset = presets.len();
    }
    ui.add_space(6.0);
    ui.label(RichText::new("Calculation Properties").strong());
    ui.checkbox(&mut p.loads.use_density, "Use density (own weight)");
    ui.horizontal(|ui| {
        ui.label("Number of shaft divisions");
        let mut n = p.loads.divisions as f64;
        if ui
            .add(ExprValue::new(&mut n).range(10.0..=5000.0))
            .changed()
        {
            p.loads.divisions = n.round() as usize;
        }
    });
    ui.horizontal(|ui| {
        ui.label("Reduced stress");
        ui.radio_value(&mut p.loads.reduced, ReducedStress::Hmh, "HMH");
        ui.radio_value(&mut p.loads.reduced, ReducedStress::Tresca, "Tresca-Guest");
    });
    ui.add_enabled(
        false,
        egui::Checkbox::new(&mut false, "Use shear displacement ratio"),
    )
    .on_disabled_hover_text("Spec 6.2-design-calculators §3.2: shear deflection comes later");
    ui.add_space(6.0);
    let len = p.design.length();
    ui.label(RichText::new("Supports").strong());
    let mut gone = None;
    for (k, s) in p.loads.supports.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt(("shaft_support", k))
                .width(70.0)
                .selected_text(match s.kind {
                    SupportKind::Fixed => "Fixed",
                    SupportKind::Free => "Free",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut s.kind, SupportKind::Fixed, "Fixed");
                    ui.selectable_value(&mut s.kind, SupportKind::Free, "Free");
                });
            ui.label("at z");
            mm(ui, &mut s.z);
            if ui.small_button("Delete").clicked() {
                gone = Some(k);
            }
        });
    }
    if let Some(k) = gone {
        p.loads.supports.remove(k);
    }
    ui.horizontal(|ui| {
        for (kind, text) in [
            (SupportKind::Fixed, "Add Fixed Support"),
            (SupportKind::Free, "Add Free Support"),
        ] {
            if ui
                .button(text)
                .on_hover_text(
                    "Three or more supports make the shaft statically indeterminate: their                      reactions follow from the deflections",
                )
                .clicked()
            {
                let z = free_place(&p.loads.supports, len);
                p.loads.supports.push(Support { z, kind });
            }
        }
    });
    ui.add_space(6.0);
    ui.label(RichText::new("Loads").strong());
    let mut gone = None;
    for (k, l) in p.loads.loads.iter_mut().enumerate() {
        ui.horizontal_wrapped(|ui| {
            match l {
                Load::Radial { z, force, angle } => {
                    ui.label(RichText::new("Force").strong());
                    ui.label("z");
                    mm(ui, z);
                    ui.label("F");
                    qty(ui, force, "N", 1.0);
                    ui.label("angle");
                    angle_field(ui, angle);
                }
                Load::Axial { z, force } => {
                    ui.label(RichText::new("Axial Force").strong());
                    ui.label("z");
                    mm(ui, z);
                    ui.label("F");
                    qty(ui, force, "N", 1.0);
                }
                Load::Continuous { z0, z1, q, angle } => {
                    ui.label(RichText::new("Continuous Load").strong());
                    ui.label("from");
                    mm(ui, z0);
                    ui.label("to");
                    mm(ui, z1);
                    ui.label("q");
                    qty(ui, q, "N/mm", 1.0);
                    ui.label("angle");
                    angle_field(ui, angle);
                }
                Load::Torque { z, torque } => {
                    ui.label(RichText::new("Torque").strong());
                    ui.label("z");
                    mm(ui, z);
                    ui.label("T");
                    qty(ui, torque, "N·m", 1000.0);
                }
                Load::Moment { z, moment, angle } => {
                    ui.label(RichText::new("Bending Moment").strong());
                    ui.label("z");
                    mm(ui, z);
                    ui.label("M");
                    qty(ui, moment, "N·m", 1000.0);
                    ui.label("plane");
                    angle_field(ui, angle);
                }
            }
            if ui.small_button("Delete").clicked() {
                gone = Some(k);
            }
        });
    }
    if let Some(k) = gone {
        p.loads.loads.remove(k);
    }
    ui.horizontal_wrapped(|ui| {
        let mid = len / 2.0;
        if ui.button("Add Force").clicked() {
            p.loads.loads.push(Load::Radial {
                z: mid,
                force: 1000.0,
                angle: 0.0,
            });
        }
        if ui.button("Add Axial Force").clicked() {
            p.loads.loads.push(Load::Axial {
                z: len,
                force: 500.0,
            });
        }
        if ui.button("Add Continuous Load").clicked() {
            p.loads.loads.push(Load::Continuous {
                z0: len * 0.4,
                z1: len * 0.6,
                q: 10.0,
                angle: 0.0,
            });
        }
        if ui
            .button("Add Torque")
            .on_hover_text("A pair: in at one place, out at another (they sum to zero)")
            .clicked()
        {
            p.loads.loads.push(Load::Torque {
                z: 0.0,
                torque: 100e3,
            });
            p.loads.loads.push(Load::Torque {
                z: len,
                torque: -100e3,
            });
        }
        if ui
            .button("Add Bending Moment")
            .on_hover_text(
                "A moment at a place along the shaft, bending it in the plane of its angle",
            )
            .clicked()
        {
            p.loads.loads.push(Load::Moment {
                z: mid,
                moment: 20e3,
                angle: 0.0,
            });
        }
    });
    ui.add_space(6.0);
    let f = &mut p.loads.fatigue;
    ui.label(RichText::new("Fatigue factors (NASA RP-1123 eq. 19)").strong());
    egui::Grid::new("shaft_fatigue")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label("Surface factor ka (Fig. 3)");
            ui.add(ExprValue::new(&mut f.surface));
            ui.end_row();
            ui.label("Reliability kc (Table III)");
            egui::ComboBox::from_id_salt("shaft_reliability")
                .selected_text(f.reliability.label())
                .show_ui(ui, |ui| {
                    for r in Reliability::ALL {
                        ui.selectable_value(&mut f.reliability, r, r.label());
                    }
                });
            ui.end_row();
            ui.label("Temperature factor kd (Table IV)");
            ui.add(ExprValue::new(&mut f.temperature));
            ui.end_row();
            ui.label("Other factors kf·kg·kh·ki");
            ui.add(ExprValue::new(&mut f.other));
            ui.end_row();
            ui.label("Stress concentration Kt");
            ui.add(ExprValue::new(&mut f.kt_bending));
            ui.end_row();
            ui.label("Notch sensitivity q (Fig. 5)");
            ui.add(ExprValue::new(&mut f.notch_sensitivity));
            ui.end_row();
            ui.label("Required safety factor");
            ui.add(ExprValue::new(&mut f.safety));
            ui.end_row();
        });
}

/// A place for a new support: the middle of the longest stretch without one.
fn free_place(supports: &[Support], len: f64) -> f64 {
    let mut z: Vec<f64> = supports.iter().map(|s| s.z).collect();
    z.extend([0.0, len]);
    z.sort_by(f64::total_cmp);
    z.windows(2)
        .max_by(|a, b| (a[1] - a[0]).total_cmp(&(b[1] - b[0])))
        .map_or(len / 2.0, |w| ((w[0] + w[1]) / 2.0).round())
}

fn calc_results(p: &mut ShaftPanel, ui: &mut egui::Ui) {
    ui.label(RichText::new("Results").strong());
    let Some((loads, design, r)) = &p.results else {
        return;
    };
    let t = super::theme::tokens();
    let r = match r {
        Ok(r) => r,
        Err(e) => {
            ui.colored_label(t.error, format!("Shaft: {e}"));
            return;
        }
    };
    let (fs, at) = r.min_safety();
    let need = loads.fatigue.safety;
    let (fs_text, fs_col) = if fs >= need {
        (format!("{fs:.2} ≥ {need:.2}: holds"), t.ok)
    } else {
        (format!("{fs:.2} < {need:.2}: too weak"), t.error)
    };
    egui::Grid::new("shaft_results_grid")
        .num_columns(2)
        .striped(true)
        .show(ui, |ui| {
            let mut row = |a: &str, b: String| {
                ui.label(a);
                ui.label(b);
                ui.end_row();
            };
            row("Length", format!("{} mm", num(design.length())));
            row("Mass", format!("{:.3} kg", r.mass));
            row(
                "Bending Stress σb",
                format!("{:.2} MPa", ShaftResults::max_abs(&r.bending_stress)),
            );
            row(
                "Torsional Stress τt",
                format!("{:.2} MPa", ShaftResults::max_abs(&r.torsion_stress)),
            );
            row(
                "Reduced Stress σred",
                format!("{:.2} MPa", ShaftResults::max_abs(&r.reduced_stress)),
            );
            row(
                "Bending Moment",
                format!("{:.2} N·m", ShaftResults::max_abs(&r.moment) / 1000.0),
            );
            row(
                "Shear Force",
                format!("{:.1} N", ShaftResults::max_abs(&r.shear)),
            );
            row(
                "Deflection",
                format!("{:.2} µm", ShaftResults::max_abs(&r.deflection) * 1000.0),
            );
            row("Angle of Twist", format!("{:.4}°", r.twist));
            row(
                "Critical Speed",
                r.critical_rpm.map_or("—".into(), |v| format!("{v:.0} rpm")),
            );
            for (k, re) in r.reactions.iter().enumerate() {
                row(
                    &format!("Support {} Reaction", k + 1),
                    format!("Fy {:.1} N, Fx {:.1} N, Fz {:.1} N", re[0], re[1], re[2]),
                );
            }
            ui.label("Fatigue Safety");
            ui.colored_label(fs_col, format!("{fs_text} at z = {at:.1} mm"));
            ui.end_row();
        });
    ui.add_space(6.0);
    ui.label(RichText::new("Formulas").strong());
    for line in [
        "σb = 32 M / (π d³), τt = 16 T / (π d³), σred = √(σ² + 3τ²) (HMH)",
        "Deflection: y'' = M / (E I), I = π (d⁴ − di⁴) / 64, y = 0 at every support (three or more: the inner reactions from the deflections)",
        "Safety: FS = π d³ / (32 √((M/σf)² + ¾ (T/σy)²)), RP-1123 eq. 17",
        "σf = ka·kb·kc·kd·ke·kf·kg·kh·ki·σ'f, eq. 19; kb eq. 21 / 22; ke = 1 / (1 + q (Kt − 1)), eq. 25",
        "Twist θ = 584 T L / (d⁴ G), eq. 32; critical speed ωc = √(g ΣWδ / ΣWδ²), eq. 33",
    ] {
        ui.add(egui::Label::new(RichText::new(line).small()).wrap());
    }
    ui.add(
        egui::Label::new(
            RichText::new(
                "Source: S. H. Loewenthal, Design of Power-Transmitting Shafts, NASA RP-1123, 1984",
            )
            .weak()
            .small(),
        )
        .wrap(),
    );
    ui.add_space(6.0);
    ui.label(RichText::new("Bending Moment").strong());
    plot(ui, &r.z, &r.moment, 1e-3, "N·m", design);
    ui.label(RichText::new("Torque").strong());
    plot(ui, &r.z, &r.torque, 1e-3, "N·m", design);
}

// --------------------------------------------------------------- Graphs tab

fn graphs_tab(p: &mut ShaftPanel, ui: &mut egui::Ui) {
    let Some((_, design, Ok(r))) = &p.results else {
        ui.label(RichText::new("The calculation has no result yet (see Calculation)").weak());
        return;
    };
    let safety: Vec<f64> = r.safety.iter().map(|s| s.min(20.0)).collect();
    for (title, v, k, unit) in [
        ("Shear Force", &r.shear, 1.0, "N"),
        ("Bending Moment", &r.moment, 1e-3, "N·m"),
        ("Torque", &r.torque, 1e-3, "N·m"),
        ("Deflection", &r.deflection, 1e3, "µm"),
        ("Reduced Stress", &r.reduced_stress, 1.0, "MPa"),
        ("Fatigue Safety Factor (shown up to 20)", &safety, 1.0, ""),
    ] {
        ui.label(RichText::new(title).strong());
        plot(ui, &r.z, v, k, unit, design);
    }
}

/// One curve along the shaft, the shaft's outline faint behind it, the
/// extremes written, the value under the cursor shown.
fn plot(ui: &mut egui::Ui, z: &[f64], v: &[f64], k: f64, unit: &str, design: &ShaftDesign) {
    let t = super::theme::tokens();
    let w = ui.available_width().max(200.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 90.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 3.0, t.field);
    let inner = rect.shrink2(Vec2::new(44.0, 12.0));
    let len = z.last().copied().unwrap_or(1.0).max(1e-9);
    let lo = v.iter().fold(0.0f64, |a, &b| a.min(b * k));
    let hi = v.iter().fold(0.0f64, |a, &b| a.max(b * k));
    let span = (hi - lo).max(1e-12);
    let x = |zz: f64| inner.left() + (zz / len) as f32 * inner.width();
    let y = |vv: f64| inner.bottom() - ((vv - lo) / span) as f32 * inner.height();
    let dmax = design
        .sections
        .iter()
        .map(|s| s.diameters().0.max(s.diameters().1))
        .fold(1.0, f64::max);
    let mut zz = 0.0;
    for s in &design.sections {
        let (a, b) = (x(zz), x(zz + s.length));
        let h = (s.diameters().0 / dmax) as f32 * inner.height() * 0.4;
        p.rect_filled(
            Rect::from_min_max(
                Pos2::new(a, inner.center().y - h / 2.0),
                Pos2::new(b, inner.center().y + h / 2.0),
            ),
            0.0,
            t.control.gamma_multiply(0.5),
        );
        zz += s.length;
    }
    p.line_segment(
        [
            Pos2::new(inner.left(), y(0.0)),
            Pos2::new(inner.right(), y(0.0)),
        ],
        Stroke::new(1.0, t.text_weak),
    );
    let pts: Vec<Pos2> = z
        .iter()
        .zip(v)
        .map(|(&zz, &vv)| Pos2::new(x(zz), y(vv * k)))
        .collect();
    p.add(egui::Shape::line(pts, Stroke::new(1.5, t.accent)));
    let font = egui::FontId::proportional(11.0);
    p.text(
        Pos2::new(rect.left() + 3.0, y(hi)),
        egui::Align2::LEFT_CENTER,
        format!("{hi:.3}"),
        font.clone(),
        t.text_weak,
    );
    if lo < 0.0 {
        p.text(
            Pos2::new(rect.left() + 3.0, y(lo)),
            egui::Align2::LEFT_CENTER,
            format!("{lo:.3}"),
            font.clone(),
            t.text_weak,
        );
    }
    p.text(
        rect.right_top() + Vec2::new(-4.0, 2.0),
        egui::Align2::RIGHT_TOP,
        unit,
        font.clone(),
        t.text_weak,
    );
    if let Some(h) = resp.hover_pos()
        && !v.is_empty()
    {
        let zz = f64::from(((h.x - inner.left()) / inner.width()).clamp(0.0, 1.0)) * len;
        let i = z.partition_point(|&a| a < zz).min(v.len() - 1);
        p.line_segment(
            [
                Pos2::new(x(z[i]), rect.top()),
                Pos2::new(x(z[i]), rect.bottom()),
            ],
            Stroke::new(1.0, t.text_weak),
        );
        p.text(
            Pos2::new(x(z[i]) + 4.0, rect.top() + 2.0),
            egui::Align2::LEFT_TOP,
            format!("z {:.1} mm: {:.4} {unit}", z[i], v[i] * k),
            font,
            t.text,
        );
    }
}

// ------------------------------------------------------------- app hooks

impl CadCraftApp {
    /// Once a frame in an assembly: the preview's scene follows its builds and
    /// goes when the panel closes; the window's OK and Cancel.
    pub(super) fn shaft_follow(&mut self, ctx: &egui::Context) {
        let key = egui::Id::new("shaft_preview_shown");
        let shown: bool = ctx.data(|d| d.get_temp(key)).unwrap_or(false);
        let Some(d) = self.part_planned.as_mut().filter(|d| is_shaft(d)) else {
            if shown {
                ctx.data_mut(|d| d.insert_temp(key, false));
                self.scene_dirty = true;
            }
            return;
        };
        let rev = d.shaft.preview.lock().map_or(0, |s| s.rev);
        let mut frame = None;
        if rev != d.shaft.seen {
            d.shaft.seen = rev;
            ctx.data_mut(|d| d.insert_temp(key, true));
            if !d.shaft.framed && d.shaft.editing.is_none() {
                d.shaft.framed = true;
                frame = Some(d.clone());
            }
            self.scene_dirty = true;
        }
        if let Some(f) = frame {
            self.shaft_frame_view(&f);
        }
        let Some(d) = self.part_planned.as_mut() else {
            return;
        };
        if std::mem::take(&mut d.shaft.cancel) {
            self.part_planned = None;
            return;
        }
        if std::mem::take(&mut d.shaft.ok) {
            let d = d.clone();
            if self.shaft_ok(&d) {
                self.part_planned = None;
            }
        }
    }

    /// Frames the first preview in the upper half of the view: the generator
    /// window lies along the bottom.
    fn shaft_frame_view(&mut self, d: &PartDialog) {
        let part = d
            .shaft
            .preview
            .lock()
            .ok()
            .and_then(|s| s.done.as_ref().and_then(|(_, r)| r.as_ref().ok().cloned()));
        let Some((lo, hi)) = part.and_then(|p| p.body.bounding_box()) else {
            return;
        };
        let place = self
            .shaft_placement(d)
            .unwrap_or_else(|_| Transform3::identity());
        let (a, b) = (place * lo, place * hi);
        let (min, max) = (a.inf(&b), a.sup(&b));
        self.camera.fit(min, max);
        let radius = nalgebra::distance(&min, &max) / 2.0;
        let (_, up) = self.camera.frame();
        // Twice as far, the box's centre raised by its radius on the screen.
        self.camera.distance *= 2.0;
        self.camera.target -= up * radius * 1.1;
        self.scene_dirty = true;
    }

    /// The preview in the scene: the built part, see-through, where OK puts it.
    pub(super) fn shaft_preview_scene(&self, g: &mut super::renderer::SceneGeometry) {
        let Some(d) = self.part_planned.as_ref().filter(|d| is_shaft(d)) else {
            return;
        };
        let part = d
            .shaft
            .preview
            .lock()
            .ok()
            .and_then(|s| s.done.as_ref().and_then(|(_, r)| r.as_ref().ok().cloned()));
        let Some(part) = part else {
            return;
        };
        let place = self
            .shaft_placement(d)
            .unwrap_or_else(|_| Transform3::identity());
        super::material::add_part_meshes_seen(
            g,
            &part,
            Some(&place),
            Some([0.35, 0.65, 1.0]),
            Some(0.55),
        );
    }

    /// The shaft part made by the Shaft Generator that occurrence `occ` of
    /// the active assembly places (a part of this file): its document and the
    /// generator's inputs.
    pub(super) fn shaft_of(&self, occ: u32) -> Option<(DocRef, ShaftDesign, Option<ShaftLoads>)> {
        let a = self.assembly()?;
        let o = a.find(occ)?;
        if o.component.library.is_some() {
            return None;
        }
        let r = o.component.base();
        match &self.project.document(r.document)?.data {
            DocumentData::Part(p) => match &p.generator {
                Some(crate::design::Generator::Shaft { design, loads }) => {
                    Some((r, design.clone(), loads.clone()))
                }
                _ => None,
            },
            _ => None,
        }
    }

    /// Edit Shaft (Inventor: Edit using Design Accelerator): the generator
    /// opens on the placed shaft's sections and loads; OK rebuilds its part.
    pub(super) fn shaft_edit(&mut self, occ: u32) {
        let Some((doc, design, loads)) = self.shaft_of(occ) else {
            return;
        };
        let mut d = PartDialog::new(AsmPlanned::Shaft);
        d.shaft.design = design;
        if let Some(l) = loads {
            d.shaft.loads = l;
            d.shaft.preset = crate::design::shaft_calc::Material::presets()
                .iter()
                .position(|m| *m == d.shaft.loads.material)
                .unwrap_or(usize::MAX);
        }
        d.shaft.editing = Some((doc, occ));
        d.active = None;
        self.part_planned = Some(d);
    }

    /// The Shaft panel's OK: the part, its document and its occurrence (one
    /// Undo step). Returns whether the panel can close.
    pub(super) fn shaft_ok(&mut self, d: &PartDialog) -> bool {
        let design = &d.shaft.design;
        let mut part = match crate::design::shaft::part(design) {
            Ok(p) => p,
            Err(e) => {
                self.file_status = Some(Err(format!("Shaft: {e}")));
                return false;
            }
        };
        part.generator = Some(crate::design::Generator::Shaft {
            design: design.clone(),
            loads: Some(d.shaft.loads.clone()),
        });
        if let Some((doc, occ)) = d.shaft.editing {
            return self.shaft_replace(part, doc, occ);
        }
        let place = match self.shaft_placement(d) {
            Ok(t) => t,
            Err(e) => {
                self.file_status = Some(Err(format!("Shaft: {e}")));
                return false;
            }
        };
        if let Err(e) = part.regenerate() {
            self.file_status = Some(Err(format!("Shaft: {e}")));
            return false;
        }
        if let Some((_, msg)) = part.feature_health.iter().next() {
            self.file_status = Some(Err(format!("Shaft: {msg}")));
            return false;
        }
        let mut trial = self.project.clone();
        let asm = self.part_id;
        if !matches!(
            trial.document(asm).map(|x| &x.data),
            Some(DocumentData::Assembly(_))
        ) {
            self.file_status = Some(Err("Shaft: no assembly is open".into()));
            return false;
        }
        let name = trial.free_name("Shaft");
        let id = trial.add_document(name.clone(), DocumentData::Part(part));
        let Some(DocumentData::Assembly(a)) = trial.document_mut(asm).map(|x| &mut x.data) else {
            return false;
        };
        let first = a.occurrences.is_empty();
        let occ = a.place(DocRef::local(id), &name, place, first);
        self.history.break_coalescing();
        self.checkpoint("Shaft");
        self.project = trial;
        self.history.break_coalescing();
        self.asm_solve(None);
        self.asm.sel = vec![occ];
        self.asm.sub.clear();
        self.scene_dirty = true;
        self.file_status = Some(Ok(format!(
            "Shaft: {name} placed ({} sections, {} mm)",
            design.sections.len(),
            num(design.length())
        )));
        true
    }

    /// Edit Shaft's OK: the part document gets the new shaft (its name, id
    /// and occurrences stay). One Undo step.
    fn shaft_replace(&mut self, mut part: PartDocument, doc: DocRef, occ: u32) -> bool {
        if let Err(e) = part.regenerate() {
            self.file_status = Some(Err(format!("Shaft: {e}")));
            return false;
        }
        if let Some((_, msg)) = part.feature_health.iter().next() {
            self.file_status = Some(Err(format!("Shaft: {msg}")));
            return false;
        }
        let mut trial = self.project.clone();
        let Some(x) = trial.document_mut(doc.document) else {
            self.file_status = Some(Err("Shaft: the shaft's part no longer exists".into()));
            return false;
        };
        let name = x.name.clone();
        x.data = DocumentData::Part(part);
        self.history.break_coalescing();
        self.checkpoint("Edit Shaft");
        self.project = trial;
        self.history.break_coalescing();
        self.asm_solve(None);
        self.asm.sel = vec![occ];
        self.asm.sub.clear();
        self.scene_dirty = true;
        self.file_status = Some(Ok(format!("Shaft: {name} rebuilt")));
        true
    }

    /// Where the shaft goes: its start on the Start plane, along the Axis
    /// (or the plane's normal), reversed by Flip; the origin without picks.
    fn shaft_placement(&self, d: &PartDialog) -> Result<Transform3, String> {
        let (t, _) = self.axis_placement(d, "axis", "start", d.check(0, "Flip"))?;
        Ok(t)
    }

    /// A placement on the axis picked in selector `axis_key` (a cylindrical
    /// face, an axis or a circular edge) starting at the plane of `start_key`,
    /// its Z along the axis (reversed by `flip`); the origin without picks.
    /// Also the picked cylinder's radius, when the axis is one.
    pub(super) fn axis_placement(
        &self,
        d: &PartDialog,
        axis_key: &str,
        start_key: &str,
        flip: bool,
    ) -> Result<(Transform3, Option<f64>), String> {
        let a = self.assembly().ok_or("no assembly")?;
        let geom = |key: &str| -> Result<Option<Geom>, String> {
            let Some(r) = d.refs.get(key).and_then(|v| v.first()) else {
                return Ok(None);
            };
            let g = self.asm_local_in(a, r)?;
            Ok(Some(if r.occurrence() == ASSEMBLY {
                g
            } else {
                let o = a
                    .find(r.occurrence())
                    .ok_or("the component no longer exists")?;
                g.moved(&o.transform)
            }))
        };
        let mut radius = None;
        let axis = match geom(axis_key)? {
            Some(Geom::Cylinder { p, d, r }) => {
                radius = Some(r);
                Some((p, d))
            }
            Some(Geom::Axis { p, d }) => Some((p, d)),
            Some(Geom::Circle { c, n, r }) => {
                radius = Some(r);
                Some((c, n))
            }
            Some(_) => return Err("the axis must be a cylindrical face, an axis or an edge".into()),
            None => None,
        };
        let plane = match geom(start_key)? {
            Some(Geom::Plane { p, n }) => Some((p, n)),
            Some(_) => return Err("the start must be a planar face or a plane".into()),
            None => None,
        };
        let (o, dir): (Point3, Vector3) = match (axis, plane) {
            (Some((p, dv)), Some((q, n))) => {
                let den = dv.dot(&n);
                if den.abs() < 1e-9 {
                    return Err("the axis lies in the start plane".into());
                }
                let t = (q - p).dot(&n) / den;
                let dir = if den < 0.0 { -dv } else { dv };
                (p + dv * t, dir)
            }
            (Some((p, dv)), None) => (p, dv),
            (None, Some((q, n))) => (q, n),
            (None, None) => (Point3::origin(), Vector3::z()),
        };
        let dir = if flip { -dir } else { dir };
        Ok((crate::assembly::create::frame_on(o, dir), radius))
    }

    /// Places `part` as a new document of the design file in the active
    /// assembly at `place`, named after `base`; returns the occurrence. The
    /// caller made the Undo checkpoint.
    pub(super) fn place_generated(
        &mut self,
        trial: &mut crate::document::Project,
        part: PartDocument,
        base: &str,
        place: Transform3,
    ) -> Result<u32, String> {
        let asm = self.part_id;
        let name = trial.free_name(base);
        let id = trial.add_document(name.clone(), DocumentData::Part(part));
        let Some(DocumentData::Assembly(a)) = trial.document_mut(asm).map(|x| &mut x.data) else {
            return Err("no assembly is open".into());
        };
        let first = a.occurrences.is_empty();
        Ok(a.place(DocRef::local(id), &name, place, first))
    }
}
