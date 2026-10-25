//! File → New (Inventor's Create New File): a window of document kinds as
//! large icon tiles — Part, Assembly, and those still to come (Sheet Metal,
//! Drawing) greyed with their section — so a new file is a choice, not a
//! fixed part (the owner, 2026-10-24).

use eframe::egui::{self, RichText};

use super::CadCraftApp;
use super::icons::Icon;
use super::ribbon::{Size, icon_button};

/// What the tiles make.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Part,
    Assembly,
    SheetMetal,
}

impl CadCraftApp {
    /// File → New / Ctrl+N: the chooser.
    pub(super) fn open_new_file(&mut self) {
        self.new_file = true;
    }

    /// The Create New File window.
    pub(super) fn new_file_window(&mut self, ctx: &egui::Context) {
        if !self.new_file {
            return;
        }
        let mut open = true;
        let mut chosen = None;
        let win = egui::Window::new("Create New File")
            .open(&mut open)
            .resizable(false)
            .collapsible(false);
        self.layout
            .place("new_file", ctx.content_rect(), false, win)
            .show(ctx, |ui| {
                ui.label("What do you want to make?");
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let tile = |ui: &mut egui::Ui, icon, label, tip: &str, enabled| {
                        ui.vertical(|ui| {
                            ui.set_width(96.0);
                            let r =
                                icon_button(ui, icon, label, Size::Large, enabled, false, false);
                            ui.label(RichText::new(tip).small().weak());
                            r.clicked() && enabled
                        })
                        .inner
                    };
                    if tile(ui, Icon::Part, "Part", "a solid part", true) {
                        chosen = Some(Kind::Part);
                    }
                    if tile(
                        ui,
                        Icon::Assembly,
                        "Assembly",
                        "parts placed together",
                        true,
                    ) {
                        chosen = Some(Kind::Assembly);
                    }
                    if tile(
                        ui,
                        Icon::SheetMetal,
                        "Sheet Metal",
                        "a part of sheet stock",
                        true,
                    ) {
                        chosen = Some(Kind::SheetMetal);
                    }
                    tile(ui, Icon::Drawing, "Drawing", "planned (4)", false);
                });
            });
        let now = ctx.input(|i| i.time);
        match chosen {
            Some(Kind::Part) => {
                self.new_file = false;
                self.request(super::files::Pending::New, now);
            }
            Some(Kind::SheetMetal) => {
                self.new_file = false;
                self.add_blank_tab();
                self.new_project();
                self.part_mut().make_sheet_metal(None);
                self.mark_saved();
                self.tab = super::ribbon::Tab::SheetMetal;
            }
            Some(Kind::Assembly) => {
                self.new_file = false;
                self.new_assembly();
            }
            None => {
                if !open {
                    self.new_file = false;
                }
            }
        }
    }
}
