//! Tools › Document Settings: Inventor's per-document settings dialog.
//! Interface first: the tabs are all there; Modeling › Tapped Hole Diameter
//! works (`docs/spec/2.3-hole.md`, Threads: Diameter), the other options are
//! shown disabled until their logic exists.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use eframe::egui::{self, RichText};

use super::CadCraftApp;
use crate::part::hole::TapDiameter;

/// The window is open.
static OPEN: AtomicBool = AtomicBool::new(false);
/// The tab shown (index into [`TABS`]).
static TAB: AtomicU8 = AtomicU8::new(3);

/// Inventor's tabs of a part's Document Settings.
const TABS: [&str; 6] = [
    "Standard",
    "Units",
    "Sketch",
    "Modeling",
    "Bill of Materials",
    "Default Tolerance",
];

/// Tools › Document Settings.
pub(super) fn open() {
    OPEN.store(true, Ordering::Relaxed);
}

fn planned(ui: &mut egui::Ui, text: &str) {
    let mut v = false;
    ui.add_enabled(false, egui::Checkbox::new(&mut v, text))
        .on_disabled_hover_text("Planned (Document Settings)");
}

impl CadCraftApp {
    pub(super) fn doc_settings_window(&mut self, ctx: &egui::Context) {
        if !OPEN.load(Ordering::Relaxed) {
            return;
        }
        let mut open = true;
        let mut tab = usize::from(TAB.load(Ordering::Relaxed)).min(TABS.len() - 1);
        let part_doc = !self.is_drawing() && !self.is_assembly();
        let current = self.part().tapped_hole_diameter;
        let mut chosen = current;
        let mut close = false;
        let win = egui::Window::new("Document Settings")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .default_width(460.0);
        self.layout
            .place("document_settings", ctx.content_rect(), false, win)
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (k, name) in TABS.iter().enumerate() {
                        ui.selectable_value(&mut tab, k, *name);
                    }
                });
                ui.separator();
                match tab {
                    3 => {
                        ui.add_enabled_ui(part_doc, |ui| {
                            ui.label(RichText::new("Tapped Hole Diameter").strong());
                            ui.horizontal(|ui| {
                                for d in TapDiameter::ALL {
                                    ui.radio_value(&mut chosen, d, d.label());
                                }
                            });
                            ui.weak(
                                "The diameter every tapped hole with a cosmetic thread is cut at (Minor: what drawings need for their thread display)",
                            );
                        })
                        .response
                        .on_disabled_hover_text("Modeling settings belong to parts");
                        ui.add_space(6.0);
                        ui.label(RichText::new("Advanced Properties").strong());
                        planned(ui, "Compute Physical Properties on Save");
                        ui.label(RichText::new("Construction").strong());
                        planned(ui, "Compact Model History");
                        ui.label(RichText::new("3D Snap Spacing").strong());
                        ui.add_enabled_ui(false, |ui| {
                            ui.horizontal(|ui| {
                                ui.label("Distance 1 mm   Angle 15 deg");
                            });
                        })
                        .response
                        .on_disabled_hover_text("Planned (Document Settings)");
                        planned(ui, "User Coordinate System: Initial view orientation");
                    }
                    _ => {
                        ui.add_enabled_ui(false, |ui| {
                            ui.label(format!(
                                "{}: these settings come with their features (planned)",
                                TABS[tab]
                            ));
                        });
                    }
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
            });
        TAB.store(u8::try_from(tab).unwrap_or(3), Ordering::Relaxed);
        if !open || close {
            OPEN.store(false, Ordering::Relaxed);
        }
        if chosen != current && part_doc {
            self.checkpoint("Document Settings");
            match self.part_mut().set_tapped_hole_diameter(chosen) {
                Ok(()) => {
                    self.scene_dirty = true;
                    self.file_status = None;
                }
                Err(e) => {
                    self.history.cancel();
                    self.file_status = Some(Err(format!("Document Settings: {e}")));
                }
            }
        }
    }
}
