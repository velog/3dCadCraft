//! **Insert AutoCAD File** in a sketch (spec 1.11a, Inventor's Sketch › Insert › ACAD): a DXF's
//! model space read by `sketch::dxf`, its layers chosen, its unit confirmed, then the geometry
//! put into the open sketch at the sketch's origin, the ends joined into profiles.
//!
//! The file is read once when it is picked; the panel shows the drawing in the view (orange)
//! as it will come in; OK adds it in one Undo step.

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Stroke};

use super::CadCraftApp;
use super::icons::Icon;
use super::panels::plabel;
use crate::kernel::Real;
use crate::sketch::dxf::{DxfDrawing, DxfGeom, InsertOptions};
use crate::sketch::{Point2, Vector2};

/// The units the panel offers, and their size in mm (index 0: as the file says).
const UNITS: [(&str, Real); 6] = [
    ("From file", 0.0),
    ("mm", 1.0),
    ("cm", 10.0),
    ("m", 1000.0),
    ("in", 25.4),
    ("ft", 304.8),
];

/// The open Insert AutoCAD File panel.
pub(super) struct DxfDraft {
    /// The file's name (shown).
    name: String,
    drawing: DxfDrawing,
    /// Each layer, taken or not, and how many items it holds.
    layers: Vec<(String, bool, usize)>,
    /// Index into [`UNITS`].
    unit: usize,
    /// Constrain End Points: ends that meet share one point.
    join: bool,
    construction: bool,
}

impl DxfDraft {
    fn new(name: String, drawing: DxfDrawing) -> Self {
        let layers = drawing
            .layers
            .iter()
            .map(|l| {
                let n = drawing.items.iter().filter(|i| &i.layer == l).count();
                (l.clone(), true, n)
            })
            .collect();
        DxfDraft {
            name,
            drawing,
            layers,
            unit: 0,
            join: true,
            construction: false,
        }
    }

    /// The scale on top of what the reader applied (its `$INSUNITS`).
    fn scale(&self) -> Real {
        match UNITS.get(self.unit) {
            Some((_, mm)) if *mm > 0.0 => mm / self.drawing.unit.max(1e-12),
            _ => 1.0,
        }
    }

    fn options(&self) -> InsertOptions {
        InsertOptions {
            layers: self
                .layers
                .iter()
                .filter(|l| l.1)
                .map(|l| l.0.clone())
                .collect(),
            scale: self.scale(),
            join: if self.join { 1e-6 } else { 0.0 },
            construction: self.construction,
            ..Default::default()
        }
    }

    /// The taken items, nothing when no layer is.
    fn taken(&self) -> impl Iterator<Item = &DxfGeom> {
        self.drawing
            .items
            .iter()
            .filter(|i| self.layers.iter().any(|l| l.1 && l.0 == i.layer))
            .map(|i| &i.geom)
    }
}

/// Points along a drawing item (sketch coordinates, before the scale).
fn polyline(g: &DxfGeom) -> Vec<Point2> {
    let arc = |c: Point2, u: Vector2, v: Vector2, a0: Real, a1: Real| -> Vec<Point2> {
        let n = (((a1 - a0).abs() / std::f64::consts::TAU) * 48.0)
            .ceil()
            .max(2.0) as usize;
        (0..=n)
            .map(|j| {
                let t = a0 + (a1 - a0) * j as Real / n as Real;
                c + u * t.cos() + v * t.sin()
            })
            .collect()
    };
    match g {
        DxfGeom::Point(p) => vec![*p],
        DxfGeom::Line(a, b) => vec![*a, *b],
        DxfGeom::Circle { c, r } => arc(
            *c,
            Vector2::new(*r, 0.0),
            Vector2::new(0.0, *r),
            0.0,
            std::f64::consts::TAU,
        ),
        DxfGeom::Arc { c, r, start, end } => {
            let sweep = (end - start).rem_euclid(std::f64::consts::TAU);
            arc(
                *c,
                Vector2::new(*r, 0.0),
                Vector2::new(0.0, *r),
                *start,
                start + sweep,
            )
        }
        DxfGeom::Ellipse {
            c,
            u,
            v,
            start,
            end,
        } => arc(*c, *u, *v, *start, *end),
        DxfGeom::Spline { control, fit, .. } => {
            if fit.len() >= 2 {
                fit.clone()
            } else {
                control.clone()
            }
        }
        DxfGeom::Text { at, .. } => vec![*at],
    }
}

impl CadCraftApp {
    /// The command: the file dialog, in an open sketch only.
    pub(super) fn insert_dxf_start(&mut self) {
        if self.editing.is_none() {
            self.file_status = Some(Err(
                "Insert AutoCAD File: open a sketch first (it inserts into the sketch)".into(),
            ));
            return;
        }
        self.open_dialog(super::files::DialogPurpose::InsertDxf);
    }

    /// A DXF file was picked: read it and open the panel.
    pub(super) fn insert_dxf_picked(&mut self, path: &std::path::Path) {
        let read = std::fs::read(path)
            .map_err(|e| format!("{}: {e}", path.display()))
            .and_then(|b| crate::sketch::dxf::read(&b));
        match read {
            Ok(d) if d.items.is_empty() => {
                self.file_status = Some(Err(
                    "Insert AutoCAD File: the drawing's model space is empty".into(),
                ));
            }
            Ok(d) => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.file_status = None;
                self.dxf_draft = Some(DxfDraft::new(name, d));
            }
            Err(e) => self.file_status = Some(Err(format!("Insert AutoCAD File: {e}"))),
        }
    }

    fn insert_dxf_ok(&mut self) {
        let (Some(d), Some(i)) = (self.dxf_draft.as_ref(), self.editing) else {
            return;
        };
        let o = d.options();
        let drawing = d.drawing.clone();
        self.checkpoint("Insert AutoCAD File");
        let rep = self.part_mut().sketches[i].sketch.insert_dxf(&drawing, &o);
        if rep.total() == 0 {
            self.history.cancel();
            self.file_status = Some(Err(
                "Insert AutoCAD File: nothing to insert on the chosen layers".into(),
            ));
            return;
        }
        self.dxf_draft = None;
        self.scene_dirty = true;
        self.solve();
        let mut msg = format!(
            "Inserted {} lines, {} arcs, {} circles, {} splines, {} points",
            rep.lines, rep.arcs, rep.circles, rep.splines, rep.points
        );
        if rep.approximated > 0 {
            msg += &format!(
                "; {} ellipses / splines as splines through their points",
                rep.approximated
            );
        }
        if rep.skipped > 0 {
            msg += &format!("; {} texts or tiny pieces left out", rep.skipped);
        }
        self.file_status = Some(Ok(msg));
    }

    /// The panel in Properties.
    pub(super) fn insert_dxf_panel(&mut self, ui: &mut egui::Ui) {
        self.panel_header(ui, Icon::InsertDxf, "Insert AutoCAD File");
        let Some(d) = self.dxf_draft.as_mut() else {
            return;
        };
        ui.label(RichText::new(&d.name).strong());
        let (lo, hi) = d.drawing.bounds().unwrap_or_default();
        let s = d.scale();
        ui.label(format!(
            "{} items, {:.2} x {:.2} mm",
            d.drawing.items.len(),
            (hi.x - lo.x) * s,
            (hi.y - lo.y) * s
        ));
        ui.add_space(4.0);
        ui.label(RichText::new("Layers").strong());
        ui.horizontal(|ui| {
            if ui.small_button("All").clicked() {
                d.layers.iter_mut().for_each(|l| l.1 = true);
            }
            if ui.small_button("None").clicked() {
                d.layers.iter_mut().for_each(|l| l.1 = false);
            }
        });
        egui::ScrollArea::vertical()
            .id_salt("dxf_layers")
            .max_height(180.0)
            .show(ui, |ui| {
                for (name, on, n) in d.layers.iter_mut() {
                    ui.checkbox(on, format!("{name}  ({n})"));
                }
            });
        ui.add_space(4.0);
        egui::Grid::new("dxf_opts").num_columns(2).show(ui, |ui| {
            plabel(ui, "Units");
            let file_unit = UNITS
                .iter()
                .skip(1)
                .find(|u| (u.1 - d.drawing.unit).abs() < 1e-9)
                .map_or("mm", |u| u.0);
            let label = |k: usize| match k {
                0 => format!("From file ({file_unit})"),
                k => UNITS[k].0.to_owned(),
            };
            egui::ComboBox::from_id_salt("dxf_unit")
                .selected_text(label(d.unit))
                .show_ui(ui, |ui| {
                    for k in 0..UNITS.len() {
                        ui.selectable_value(&mut d.unit, k, label(k));
                    }
                })
                .response
                .on_hover_text(
                    "The drawing's length unit: the file's $INSUNITS, or the one chosen here",
                );
            ui.end_row();
        });
        ui.checkbox(&mut d.join, "Constrain End Points")
            .on_hover_text("Ends that meet share one point, so closed outlines are profiles");
        ui.checkbox(&mut d.construction, "Construction")
            .on_hover_text("Bring the geometry in as construction lines");
        if d.drawing
            .items
            .iter()
            .any(|i| matches!(i.geom, DxfGeom::Text { .. }))
        {
            ui.label(
                RichText::new("Texts are left out (not inserted yet)")
                    .color(super::theme::tokens().warning),
            );
        }
        let ready = d.layers.iter().any(|l| l.1);
        let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
        let r = super::theme::panel_footer(ui, |ui| {
            let ok = ui.add_enabled(ready, egui::Button::new("OK")).clicked()
                || super::keys::panel_ok_key(ui, ready);
            (ok, ui.button("Cancel").clicked())
        });
        let (ok, cancel) = r.inner;
        if ok {
            self.insert_dxf_ok();
        } else if cancel || esc {
            self.dxf_draft = None;
        }
    }

    /// The drawing in the view as it will come in.
    pub(super) fn insert_dxf_preview(&self, painter: &egui::Painter, rect: Rect) {
        let (Some(d), Some(i)) = (self.dxf_draft.as_ref(), self.editing) else {
            return;
        };
        let Some(ps) = self.part().sketches.get(i) else {
            return;
        };
        let s = d.scale();
        let stroke = Stroke::new(1.5, Color32::from_rgb(235, 130, 40));
        for g in d.taken() {
            let pts: Vec<Pos2> = polyline(g)
                .iter()
                .filter_map(|p| {
                    let w = ps.sketch.plane.point_at(p.x * s, p.y * s);
                    self.camera.project(&w, rect)
                })
                .collect();
            if pts.len() == 1 {
                painter.circle_filled(pts[0], 2.5, stroke.color);
            } else {
                painter.add(egui::Shape::line(pts, stroke));
            }
        }
    }
}
