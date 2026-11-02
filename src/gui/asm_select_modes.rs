//! The assembly's component selection modes with an input (spec 0.9a §6b):
//! Component Size, Component Offset, Sphere Offset, Select by Plane, External,
//! Internal and All in Camera. One small window; the selection follows its
//! values live, OK keeps it, Cancel brings the old one back.

use std::sync::{Arc, Mutex};

use eframe::egui;

use super::CadCraftApp;
use super::valuefield::ExprValue;
use crate::assembly::select::{self as sel, CompBox, RayPart};
use crate::kernel::{Point3, Vector3};

/// Rays per side of the assembly's box for External / Internal.
const RAY_GRID: usize = 24;

/// Which mode is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SelMode {
    Size,
    Offset,
    Sphere,
    Plane,
    External,
    Internal,
    Camera,
}

impl SelMode {
    pub(super) fn title(self) -> &'static str {
        match self {
            SelMode::Size => "Component Size",
            SelMode::Offset => "Component Offset",
            SelMode::Sphere => "Sphere Offset",
            SelMode::Plane => "Select by Plane",
            SelMode::External => "External Components",
            SelMode::Internal => "Internal Components",
            SelMode::Camera => "All in Camera",
        }
    }
}

/// The open mode's values.
#[derive(Clone)]
pub(super) struct SelModeDraft {
    pub mode: SelMode,
    /// Size (mm, or percent of the largest).
    pub size: f64,
    pub percent_size: bool,
    pub at_least: bool,
    /// Offset of the box, radius of the sphere, position of the plane (mm).
    pub value: f64,
    pub partial: bool,
    /// The plane's axis: 0 X, 1 Y, 2 Z.
    pub axis: usize,
    pub flip: bool,
    /// External, Internal, All in Camera (%).
    pub percent: f64,
    /// The selection when the window opened (Cancel, and the box's source).
    pub before: Vec<u32>,
    /// What can be seen of each component, once the rays are back.
    pub seen: Arc<Mutex<Option<std::collections::HashMap<u32, f64>>>>,
    /// The values the selection was last made for.
    pub key: String,
}

impl CadCraftApp {
    /// Every top-level occurrence's box (assembly coordinates).
    fn comp_boxes(&self) -> Vec<CompBox> {
        let visible_only = self.select.asm_visible_only;
        let shown: Vec<u32> = self.assembly().map_or_else(Vec::new, |a| {
            a.occurrences
                .iter()
                .filter(|o| o.shown() || !visible_only)
                .map(|o| o.id)
                .collect()
        });
        let mut out: Vec<CompBox> = Vec::new();
        for part in self.pick_parts() {
            if !shown.contains(&part.top) {
                continue;
            }
            let Some(Some((a, b))) = self.with_part(part.component, |p| p.body.bounding_box())
            else {
                continue;
            };
            for k in 0..8 {
                let c = Point3::new(
                    if k & 1 == 0 { a.x } else { b.x },
                    if k & 2 == 0 { a.y } else { b.y },
                    if k & 4 == 0 { a.z } else { b.z },
                );
                let w = part.place * c;
                match out.iter_mut().find(|x| x.id == part.top) {
                    Some(x) => {
                        x.lo = x.lo.inf(&w);
                        x.hi = x.hi.sup(&w);
                    }
                    None => out.push(CompBox {
                        id: part.top,
                        lo: w,
                        hi: w,
                    }),
                }
            }
        }
        out
    }

    /// Opens a mode; External / Internal send their rays to a worker thread
    /// (rule 20: never in the frame).
    pub(super) fn asm_open_sel_mode(&mut self, mode: SelMode) {
        let seen = Arc::new(Mutex::new(None));
        if matches!(mode, SelMode::External | SelMode::Internal) {
            let parts: Vec<RayPart> = self
                .pick_parts()
                .into_iter()
                .filter_map(|p| {
                    let mesh = self.asm_mesh(p.component)?;
                    let (a, b) = self
                        .with_part(p.component, |x| x.body.bounding_box())
                        .flatten()?;
                    let mut lo = p.place * a;
                    let mut hi = lo;
                    for k in 0..8 {
                        let c = Point3::new(
                            if k & 1 == 0 { a.x } else { b.x },
                            if k & 2 == 0 { a.y } else { b.y },
                            if k & 4 == 0 { a.z } else { b.z },
                        );
                        let w = p.place * c;
                        lo = lo.inf(&w);
                        hi = hi.sup(&w);
                    }
                    Some(RayPart {
                        top: p.top,
                        place: p.place,
                        mesh,
                        lo,
                        hi,
                    })
                })
                .collect();
            let out = seen.clone();
            std::thread::spawn(move || {
                let r = sel::exposure(&parts, RAY_GRID);
                if let Ok(mut g) = out.lock() {
                    *g = Some(r);
                }
            });
        }
        let before = self.asm.sel.clone();
        self.asm.sel_mode = Some(SelModeDraft {
            mode,
            size: 100.0,
            percent_size: true,
            at_least: true,
            value: 0.0,
            partial: false,
            axis: 2,
            flip: false,
            percent: if mode == SelMode::Camera { 100.0 } else { 5.0 },
            before,
            seen,
            key: String::new(),
        });
    }

    /// What the mode selects with its values now; `None` while the rays are out.
    fn sel_mode_result(&self, d: &SelModeDraft, view: egui::Rect) -> Option<Vec<u32>> {
        let boxes = self.comp_boxes();
        let around = || {
            sel::union_of(
                &boxes,
                &if d.before.is_empty() {
                    boxes.iter().map(|b| b.id).collect::<Vec<u32>>()
                } else {
                    d.before.clone()
                },
            )
            .unwrap_or((Point3::origin(), Point3::origin()))
        };
        Some(match d.mode {
            SelMode::Size => {
                let size = if d.percent_size {
                    sel::largest(&boxes) * d.size / 100.0
                } else {
                    d.size
                };
                sel::by_size(&boxes, size, d.at_least)
            }
            SelMode::Offset => {
                let (lo, hi) = around();
                sel::in_box(&boxes, lo, hi, d.value, d.partial)
            }
            SelMode::Sphere => {
                let (lo, hi) = around();
                let r = (hi - lo).norm() / 2.0 + d.value;
                sel::in_sphere(&boxes, nalgebra::center(&lo, &hi), r, d.partial)
            }
            SelMode::Plane => {
                let mut n = Vector3::zeros();
                n[d.axis] = if d.flip { -1.0 } else { 1.0 };
                let mut p = Point3::origin();
                p[d.axis] = d.value;
                sel::by_plane(&boxes, p, n)
            }
            SelMode::External | SelMode::Internal => {
                let g = d.seen.lock().ok()?;
                let seen = g.as_ref()?;
                let mut v = if d.mode == SelMode::External {
                    sel::external(seen, d.percent)
                } else {
                    sel::internal(seen, d.percent)
                };
                v.retain(|id| boxes.iter().any(|b| b.id == *id));
                v
            }
            SelMode::Camera => boxes
                .iter()
                .filter(|b| {
                    let mut r: Option<[f32; 4]> = None;
                    for k in 0..8 {
                        let c = Point3::new(
                            if k & 1 == 0 { b.lo.x } else { b.hi.x },
                            if k & 2 == 0 { b.lo.y } else { b.hi.y },
                            if k & 4 == 0 { b.lo.z } else { b.hi.z },
                        );
                        if let Some(q) = self.camera.project(&c, view) {
                            r = Some(match r {
                                Some(a) => {
                                    [a[0].min(q.x), a[1].min(q.y), a[2].max(q.x), a[3].max(q.y)]
                                }
                                None => [q.x, q.y, q.x, q.y],
                            });
                        }
                    }
                    let v = [view.left(), view.top(), view.right(), view.bottom()];
                    r.is_some_and(|a| f64::from(sel::share_in_view(a, v)) * 100.0 >= d.percent)
                })
                .map(|b| b.id)
                .collect(),
        })
    }

    /// The window of the open mode.
    pub(super) fn asm_sel_mode_window(&mut self, ctx: &egui::Context, view: egui::Rect) {
        let Some(mut d) = self.asm.sel_mode.clone() else {
            return;
        };
        let (mut ok, mut cancel) = (false, false);
        let mut open = true;
        let pending = matches!(d.mode, SelMode::External | SelMode::Internal)
            && d.seen.lock().is_ok_and(|g| g.is_none());
        self.layout
            .place(
                "asm_sel_mode",
                ctx.content_rect(),
                false,
                egui::Window::new(d.mode.title())
                    .open(&mut open)
                    .collapsible(false)
                    .resizable(false),
            )
            .show(ctx, |ui| {
                match d.mode {
                    SelMode::Size => {
                        ui.horizontal(|ui| {
                            ui.radio_value(&mut d.at_least, true, "At Least");
                            ui.radio_value(&mut d.at_least, false, "At Most");
                        });
                        ui.horizontal(|ui| {
                            ui.label("Size");
                            let unit = if d.percent_size { " %" } else { " mm" };
                            ui.add(ExprValue::new(&mut d.size).range(0.0..=1e7).suffix(unit));
                            ui.checkbox(&mut d.percent_size, "%").on_hover_text(
                                "A share of the largest component (100 % = the largest)",
                            );
                        });
                    }
                    SelMode::Offset | SelMode::Sphere => {
                        ui.horizontal(|ui| {
                            ui.label(if d.mode == SelMode::Offset {
                                "Offset"
                            } else {
                                "Added radius"
                            });
                            ui.add(ExprValue::new(&mut d.value).suffix(" mm"));
                        });
                        ui.checkbox(&mut d.partial, "Include Partially Contained");
                        ui.label(
                            egui::RichText::new(if d.before.is_empty() {
                                "Around the whole assembly: select components first to measure from them"
                            } else {
                                "Around the components selected before"
                            })
                            .small()
                            .weak(),
                        );
                    }
                    SelMode::Plane => {
                        ui.horizontal(|ui| {
                            ui.label("Plane");
                            ui.radio_value(&mut d.axis, 0, "YZ");
                            ui.radio_value(&mut d.axis, 1, "XZ");
                            ui.radio_value(&mut d.axis, 2, "XY");
                        });
                        ui.horizontal(|ui| {
                            ui.label("Position");
                            ui.add(ExprValue::new(&mut d.value).suffix(" mm"));
                        });
                        ui.checkbox(&mut d.flip, "Flip Direction");
                    }
                    SelMode::External | SelMode::Internal | SelMode::Camera => {
                        ui.horizontal(|ui| {
                            ui.label("Percent");
                            ui.add(
                                ExprValue::new(&mut d.percent)
                                    .range(0.0..=100.0)
                                    .suffix(" %"),
                            );
                        });
                        if pending {
                            ui.label("Computing...");
                        }
                    }
                }
                ui.label(format!("{} component(s)", self.asm.sel.len()));
                ui.horizontal(|ui| {
                    ok = ui.button("OK").clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });
        let key = format!(
            "{} {} {} {} {} {} {} {} {pending}",
            d.size, d.percent_size, d.at_least, d.value, d.partial, d.axis, d.flip, d.percent
        );
        if key != d.key {
            if let Some(v) = self.sel_mode_result(&d, view) {
                self.asm.sel = v;
                self.asm.sub.clear();
                self.scene_dirty = true;
            }
            d.key = key;
        }
        if pending {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        if cancel || !open {
            self.asm.sel = d.before;
            self.scene_dirty = true;
            self.asm.sel_mode = None;
        } else if ok {
            self.asm.prev_sel = d.before;
            self.asm.sel_mode = None;
        } else {
            self.asm.sel_mode = Some(d);
        }
    }
}
