//! Linked parameters (spec 3.10 §3.9) in the app: the Parameters dialog's
//! Link... window — parameters of another part of this design file (a master
//! parameter part) or rows of a CSV sheet become linked user parameters of
//! the active part — and their Break Link.

use eframe::egui;

use super::CadCraftApp;
use crate::document::{DocRef, DocumentData, DocumentId};
use crate::part::params::ParamLink;

/// The Link window's state.
#[derive(Default)]
pub(super) struct LinkUi {
    /// Shown.
    pub open: bool,
    /// The chosen source part of this file.
    pub source: Option<DocumentId>,
    /// The CSV file's path as typed.
    pub csv: String,
    /// Read from the source: (name, value, unit, ticked).
    pub rows: Vec<(String, crate::kernel::Real, crate::part::expr::Unit, bool)>,
    /// What the rows were read from (`None`: the part).
    pub from_csv: bool,
}

impl CadCraftApp {
    /// The Link... window (one Undo step per Link / Break Link).
    pub(super) fn param_link_window(&mut self, ctx: &egui::Context) {
        if !self.param_link.open {
            return;
        }
        let parts: Vec<(DocumentId, String)> = self
            .project
            .documents()
            .iter()
            .filter(|d| d.id != self.part_id && matches!(d.data, DocumentData::Part(_)))
            .map(|d| (d.id, d.name.clone()))
            .collect();
        let linked: Vec<(String, String)> = self
            .part()
            .user_params
            .iter()
            .filter_map(|u| u.link.as_ref().map(|l| (u.name.clone(), l.describe())))
            .collect();
        let mut open = true;
        let mut read_part: Option<DocumentId> = None;
        let mut read_csv = false;
        let mut link_now = false;
        let mut break_one: Option<String> = None;
        egui::Window::new("Link Parameters")
            .open(&mut open)
            .resizable(true)
            .default_size([420.0, 420.0])
            .show(ctx, |ui| {
                ui.label("Parameters of another part of this file (a master parameter part):");
                let current = self
                    .param_link
                    .source
                    .and_then(|s| parts.iter().find(|p| p.0 == s))
                    .map_or("Choose a part", |p| p.1.as_str())
                    .to_owned();
                egui::ComboBox::from_id_salt("param_link_source")
                    .selected_text(current)
                    .show_ui(ui, |ui| {
                        for (id, name) in &parts {
                            if ui
                                .selectable_label(self.param_link.source == Some(*id), name)
                                .clicked()
                            {
                                read_part = Some(*id);
                            }
                        }
                    });
                ui.separator();
                ui.label("Or the rows of a CSV sheet (name, value, unit):");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.param_link.csv).desired_width(260.0),
                    );
                    if ui.button("Read").clicked() {
                        read_csv = true;
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(220.0)
                    .show(ui, |ui| {
                        for (name, v, u, on) in &mut self.param_link.rows {
                            ui.checkbox(
                                on,
                                format!("{name} = {}", crate::part::params::format_value(*v, *u)),
                            );
                        }
                    });
                let any = self.param_link.rows.iter().any(|r| r.3);
                if ui.add_enabled(any, egui::Button::new("Link")).clicked() {
                    link_now = true;
                }
                if !linked.is_empty() {
                    ui.separator();
                    ui.label("Linked here:");
                    for (n, from) in &linked {
                        ui.horizontal(|ui| {
                            ui.label(format!("{n} ({from})"));
                            if ui
                                .button("Break Link")
                                .on_hover_text(
                                    "It keeps its value and no longer follows its source",
                                )
                                .clicked()
                            {
                                break_one = Some(n.clone());
                            }
                        });
                    }
                }
            });
        self.param_link.open = open;
        if let Some(id) = read_part {
            self.param_link.source = Some(id);
            self.param_link.from_csv = false;
            self.param_link.rows = match self.project.document(id).map(|d| &d.data) {
                Some(DocumentData::Part(p)) => p
                    .user_params
                    .iter()
                    .map(|u| (u.name.clone(), u.value, u.unit, false))
                    .collect(),
                _ => Vec::new(),
            };
        }
        if read_csv {
            match std::fs::read_to_string(self.param_link.csv.trim()) {
                Ok(t) => {
                    self.param_link.from_csv = true;
                    self.param_link.rows = crate::part::params::csv_parameters(&t)
                        .into_iter()
                        .map(|(n, v, u)| (n, v, u, false))
                        .collect();
                }
                Err(e) => self.file_status = Some(Err(format!("Link: {e}"))),
            }
        }
        if link_now {
            self.param_link_apply();
        }
        if let Some(n) = break_one {
            self.checkpoint("Break Link");
            self.part_mut().break_param_link(&n);
            self.history.break_coalescing();
        }
    }

    /// Link: the ticked rows become linked user parameters (one Undo step).
    fn param_link_apply(&mut self) {
        let csv = self.param_link.csv.trim().to_owned();
        let source = self.param_link.source;
        let from_csv = self.param_link.from_csv;
        let rows: Vec<_> = self
            .param_link
            .rows
            .iter()
            .filter(|r| r.3)
            .cloned()
            .collect();
        self.checkpoint("Link Parameters");
        let mut made = Vec::new();
        for (name, v, u, _) in rows {
            let link = if from_csv {
                ParamLink::Csv {
                    path: csv.clone(),
                    name: name.clone(),
                }
            } else {
                let Some(s) = source else { continue };
                ParamLink::Document {
                    source: DocRef::local(s),
                    name: name.clone(),
                }
            };
            match self.part_mut().add_linked_parameter(&name, v, u, link) {
                Ok(n) => made.push(n),
                Err(e) => {
                    self.history.cancel();
                    self.file_status = Some(Err(format!("Link: {e}")));
                    return;
                }
            }
        }
        for r in &mut self.param_link.rows {
            r.3 = false;
        }
        self.file_status = Some(Ok(format!("Link: {} linked", made.join(", "))));
    }
}
