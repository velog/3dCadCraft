//! Drive (Inventor's Drive Constraint, `docs/spec/3.1-assemblies.md` §3.2):
//! animates one constraint's or joint's value from Start to End in steps —
//! forward, reverse, pause, to the start / end, repetitions (Start/End or
//! Start/End/Start) — solving the assembly at every step, so a mechanism
//! moves. Closing the window puts the value and the components back.

use eframe::egui::{self, RichText};

use super::CadCraftApp;
use super::valuefield::ExprValue;
use crate::assembly::relation::ConstraintKind;
use crate::kernel::{Real, Transform3};

/// The open Drive window.
pub(super) struct DriveWin {
    id: u32,
    start: Real,
    end: Real,
    steps: u32,
    /// The step shown (0..=steps).
    k: u32,
    /// +1 forward, -1 back, 0 paused.
    dir: i32,
    /// Seconds between steps.
    pause: f64,
    last: f64,
    repetitions: u32,
    done: u32,
    /// Start → End → Start counts as one repetition.
    back_and_forth: bool,
    /// The value, lock and placements before driving.
    saved: (Real, bool, Vec<(u32, Transform3)>),
    angle: bool,
}

impl CadCraftApp {
    /// Drive for constraint / joint `id` (one with a value).
    pub(super) fn open_drive(&mut self, id: u32) {
        let Some(a) = self.assembly() else {
            return;
        };
        let Some(c) = a.constraint(id).cloned() else {
            return;
        };
        let angle = matches!(c.kind, ConstraintKind::Angle | ConstraintKind::Rotational);
        let has_value = !c.kind.is_joint() || c.kind.joint_value().is_some();
        if !has_value {
            self.file_status = Some(Err(format!(
                "Drive: a {} joint has no value to drive",
                c.kind.label()
            )));
            return;
        }
        let saved = (
            c.value,
            c.locked,
            a.occurrences.iter().map(|o| (o.id, o.transform)).collect(),
        );
        let span = if angle {
            std::f64::consts::FRAC_PI_2
        } else {
            20.0
        };
        self.drive = Some(DriveWin {
            id,
            start: c.min.unwrap_or(c.value),
            end: c.max.unwrap_or(c.value + span),
            steps: 30,
            k: 0,
            dir: 0,
            pause: 0.03,
            last: 0.0,
            repetitions: 1,
            done: 0,
            back_and_forth: false,
            saved,
            angle,
        });
        self.history.break_coalescing();
    }

    /// Sets the driven value and solves (no Undo step: closing restores).
    fn drive_to(&mut self, id: u32, value: Real) -> bool {
        if let Some(c) = self.assembly_mut().and_then(|a| a.constraint_mut(id)) {
            c.value = value;
            if c.kind.is_joint() {
                c.locked = true;
            }
        }
        self.asm_solve(None);
        self.cons.last.converged
    }

    /// The Drive window and its animation.
    pub(super) fn drive_window(&mut self, ctx: &egui::Context) {
        let Some(mut w) = self.drive.take() else {
            return;
        };
        let Some(name) = self
            .assembly()
            .and_then(|a| a.constraint(w.id))
            .map(|c| c.name.clone())
        else {
            return;
        };
        let now = ctx.input(|i| i.time);
        let mut open = true;
        let mut jump: Option<u32> = None;
        let unit = |v: Real| if w.angle { v.to_degrees() } else { v };
        let sfx = if w.angle { "°" } else { " mm" };
        let win = egui::Window::new(format!("Drive ({name})"))
            .id(egui::Id::new("drive_window"))
            .open(&mut open)
            .resizable(false)
            .collapsible(false);
        self.layout
            .place("drive", ctx.content_rect(), false, win)
            .show(ctx, |ui| {
                egui::Grid::new("drive_grid")
                    .num_columns(2)
                    .min_col_width(super::panels::label_width(ui.ctx()))
                    .show(ui, |ui| {
                        for (label, slot) in [("Start", &mut w.start), ("End", &mut w.end)] {
                            super::panels::plabel(ui, label);
                            let mut v = unit(*slot);
                            let before = v;
                            ui.add(ExprValue::new(&mut v).suffix(sfx));
                            if v != before {
                                *slot = if w.angle { v.to_radians() } else { v };
                            }
                            ui.end_row();
                        }
                        super::panels::plabel(ui, "Steps");
                        let mut s = w.steps as Real;
                        ui.add(ExprValue::new(&mut s).range(1.0..=10_000.0));
                        w.steps = s.round().max(1.0) as u32;
                        ui.end_row();
                        super::panels::plabel(ui, "Pause (s)");
                        ui.add(ExprValue::new(&mut w.pause).range(0.0..=5.0));
                        ui.end_row();
                        super::panels::plabel(ui, "Repetitions");
                        let mut r = w.repetitions as Real;
                        ui.add(ExprValue::new(&mut r).range(1.0..=1000.0));
                        w.repetitions = r.round().max(1.0) as u32;
                        ui.end_row();
                    });
                ui.checkbox(&mut w.back_and_forth, "Start/End/Start")
                    .on_hover_text("One repetition goes to the end and back");
                ui.horizontal(|ui| {
                    if ui.button("|<").on_hover_text("To the start").clicked() {
                        w.dir = 0;
                        jump = Some(0);
                    }
                    if ui.button("<").on_hover_text("Reverse").clicked() {
                        w.dir = -1;
                    }
                    if ui.button("||").on_hover_text("Pause").clicked() {
                        w.dir = 0;
                    }
                    if ui.button(">").on_hover_text("Forward").clicked() {
                        if w.k >= w.steps {
                            w.k = 0;
                            w.done = 0;
                        }
                        w.dir = 1;
                    }
                    if ui.button(">|").on_hover_text("To the end").clicked() {
                        w.dir = 0;
                        jump = Some(w.steps);
                    }
                });
                let v = w.start + (w.end - w.start) * w.k as Real / w.steps as Real;
                ui.label(
                    RichText::new(format!(
                        "{:.3}{sfx}  (step {} of {})",
                        unit(v),
                        w.k,
                        w.steps
                    ))
                    .monospace(),
                );
                ui.weak("Closing the window puts the components back.");
            });
        // One step per pause while playing.
        if w.dir != 0 && now - w.last >= w.pause {
            w.last = now;
            let next = w.k as i64 + w.dir as i64;
            if next > w.steps as i64 || next < 0 {
                // An end: turn round (Start/End/Start) or start again.
                if w.back_and_forth && w.dir > 0 {
                    w.dir = -1;
                } else {
                    w.done += 1;
                    if w.done >= w.repetitions {
                        w.dir = 0;
                    } else {
                        w.k = 0;
                        w.dir = 1;
                    }
                }
            } else {
                jump = Some(next as u32);
            }
        }
        if let Some(k) = jump {
            w.k = k;
            let v = w.start + (w.end - w.start) * k as Real / w.steps as Real;
            if !self.drive_to(w.id, v) {
                w.dir = 0;
                self.file_status = Some(Err(format!(
                    "Drive: the constraints cannot hold at {:.3}{sfx}",
                    unit(v)
                )));
            }
        }
        if w.dir != 0 {
            ctx.request_repaint();
        }
        if open {
            self.drive = Some(w);
        } else {
            // Put everything back as it was.
            let (value, locked, places) = w.saved;
            if let Some(a) = self.assembly_mut() {
                if let Some(c) = a.constraint_mut(w.id) {
                    c.value = value;
                    c.locked = locked;
                }
                for (id, t) in places {
                    if let Some(o) = a.find_mut(id) {
                        o.transform = t;
                    }
                }
            }
            self.asm_refresh_status();
            self.scene_dirty = true;
        }
    }
}
