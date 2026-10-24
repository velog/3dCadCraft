//! The iProperties window (`docs/spec/0.16-iproperties.md`): General,
//! Summary, Project, Status, Custom and Physical tabs for any document of
//! the project — or for a linked part file of an assembly: its changes are
//! kept until Apply (or closing the window) and then written into the part
//! file (its open tab, if it is open).

use std::path::PathBuf;
use std::rc::Rc;

use eframe::egui::{self, Color32, RichText};

use super::CadCraftApp;
use super::valuefield::ExprValue;
use crate::document::iprops::{self, DesignState, IProperties, PropValue, STANDARD, Tab};
use crate::document::{DocumentId, Project};

/// The open window.
pub(super) struct IPropsWin {
    doc: DocumentId,
    /// A linked part file (its path and project as loaded).
    linked: Option<(PathBuf, Rc<Project>)>,
    /// Changes to a linked part file not written yet.
    pending: Option<IProperties>,
    tab: usize,
    new_name: String,
    new_type: usize,
    new_text: String,
    new_number: f64,
    new_yes: bool,
    error: Option<String>,
}

const TABS: [&str; 6] = [
    "General", "Summary", "Project", "Status", "Custom", "Physical",
];
const TYPES: [&str; 4] = ["Text", "Date", "Number", "Yes or No"];

impl IPropsWin {
    fn new(doc: DocumentId, linked: Option<(PathBuf, Rc<Project>)>) -> Self {
        Self {
            doc,
            linked,
            pending: None,
            tab: 1,
            new_name: String::new(),
            new_type: 0,
            new_text: String::new(),
            new_number: 0.0,
            new_yes: true,
            error: None,
        }
    }
}

impl CadCraftApp {
    /// File → iProperties, or a document's row in the browser.
    pub(super) fn open_iproperties(&mut self, doc: DocumentId) {
        self.iprops = Some(IPropsWin::new(doc, None));
    }

    /// iProperties of an assembly component: its document, or (read only)
    /// the linked part file.
    pub(super) fn open_component_iproperties(&mut self, occurrence: u32) {
        let Some(r) = self
            .assembly()
            .and_then(|a| a.find(occurrence))
            .map(|o| o.component)
        else {
            return;
        };
        match r.library {
            None => self.open_iproperties(r.document),
            Some(lib) => {
                let linked = self
                    .library_path(lib)
                    .and_then(|p| Some((p.clone(), self.part_file_pub(&p).ok()?)));
                self.iprops = Some(IPropsWin::new(r.document, linked));
            }
        }
    }

    /// The window (one per app; it shows one document).
    pub(super) fn iprops_window(&mut self, ctx: &egui::Context) {
        let Some(mut w) = self.iprops.take() else {
            return;
        };
        let (props, name, kind) = match &w.linked {
            Some((_, p)) => match p.document(w.doc) {
                Some(d) => (
                    w.pending.clone().unwrap_or_else(|| d.properties.clone()),
                    d.name.clone(),
                    d.data.kind(),
                ),
                None => return,
            },
            None => match self.project.document(w.doc) {
                Some(d) => (d.properties.clone(), d.name.clone(), d.data.kind()),
                None => return,
            },
        };
        let path: Option<PathBuf> = match &w.linked {
            Some((p, _)) => Some(p.clone()),
            None => {
                Some(PathBuf::from(self.file_path.trim())).filter(|p| !p.as_os_str().is_empty())
            }
        };
        let file_name = path
            .as_ref()
            .and_then(|p| p.file_stem())
            .and_then(|s| s.to_str())
            .map_or_else(|| name.clone(), str::to_owned);
        let linked = w.linked.is_some();
        let mut edited = props.clone();
        let mut apply = false;
        let mut open = true;
        let mut physical = false;
        let title = format!("{file_name} iProperties");
        let win = egui::Window::new(title)
            .id(egui::Id::new("iproperties"))
            .open(&mut open)
            .resizable(true)
            .collapsible(false)
            .default_width(460.0);
        self.layout
            .place("iproperties", ctx.content_rect(), true, win)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (k, t) in TABS.iter().enumerate() {
                        ui.selectable_value(&mut w.tab, k, *t);
                    }
                });
                ui.separator();
                if linked {
                    ui.horizontal(|ui| {
                        ui.colored_label(
                            Color32::from_rgb(200, 120, 20),
                            "A linked part file: Apply writes the changes into it.",
                        );
                        if ui
                            .add_enabled(w.pending.is_some(), egui::Button::new("Apply"))
                            .clicked()
                        {
                            apply = true;
                        }
                    });
                }
                ui.vertical(|ui| match w.tab {
                    0 => general_tab(ui, &name, kind, path.as_deref()),
                    1 => standard_tab(ui, &mut edited, Tab::Summary, &file_name),
                    2 => standard_tab(ui, &mut edited, Tab::Project, &file_name),
                    3 => {
                        standard_tab(ui, &mut edited, Tab::Status, &file_name);
                        ui.horizontal(|ui| {
                            super::panels::plabel(ui, "Design State");
                            egui::ComboBox::from_id_salt("design_state")
                                .selected_text(edited.design_state.label())
                                .show_ui(ui, |ui| {
                                    for s in DesignState::ALL {
                                        ui.selectable_value(&mut edited.design_state, s, s.label());
                                    }
                                });
                        });
                    }
                    4 => custom_tab(ui, &mut edited, &mut w),
                    _ => {
                        ui.label("Mass, area, volume, centre of gravity and inertia of the part.");
                        if ui.button("Physical Properties...").clicked() {
                            physical = true;
                        }
                    }
                });
                if let Some(e) = &w.error {
                    ui.colored_label(Color32::from_rgb(235, 80, 70), format!("iProperties: {e}"));
                }
            });
        if edited != props {
            if linked {
                w.pending = Some(edited);
            } else {
                self.checkpoint("iProperties");
                if let Some(d) = self.project.document_mut(w.doc) {
                    d.properties = edited;
                }
            }
        }
        // A linked part file: written on Apply or when the window closes.
        if (apply || !open)
            && let (Some(p), Some((path, _))) = (w.pending.take(), w.linked.clone())
        {
            match self.set_linked_props(&path, w.doc, p) {
                Ok((msg, project)) => {
                    w.linked = Some((path, project));
                    self.file_status = Some(Ok(msg));
                }
                Err(e) => self.file_status = Some(Err(format!("iProperties: {e}"))),
            }
        }
        if physical {
            self.iprops = None;
            self.apply(super::ribbon::Action::Material(
                super::material::MatCmd::Physical,
            ));
            return;
        }
        if open {
            self.iprops = Some(w);
        } else {
            self.history.break_coalescing();
        }
    }
}

fn general_tab(
    ui: &mut egui::Ui,
    name: &str,
    kind: crate::document::DocumentKind,
    path: Option<&std::path::Path>,
) {
    egui::Grid::new("ip_general")
        .num_columns(2)
        .min_col_width(super::panels::label_width(ui.ctx()))
        .show(ui, |ui| {
            super::panels::plabel(ui, "Name");
            ui.label(name);
            ui.end_row();
            super::panels::plabel(ui, "Type");
            ui.label(match kind {
                crate::document::DocumentKind::Part => "Part",
                crate::document::DocumentKind::Assembly => "Assembly",
            });
            ui.end_row();
            super::panels::plabel(ui, "Location");
            ui.label(path.map_or_else(|| "Not saved yet".to_owned(), |p| p.display().to_string()));
            ui.end_row();
            if let Some(m) = path.and_then(|p| std::fs::metadata(p).ok()) {
                super::panels::plabel(ui, "Size");
                ui.label(format!("{:.1} KB", m.len() as f64 / 1024.0));
                ui.end_row();
                if let Some(days) = m
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() / 86_400)
                {
                    super::panels::plabel(ui, "Modified");
                    ui.label(iprops::date_of_days_pub(days as i64));
                    ui.end_row();
                }
            }
        });
}

/// The text fields of one tab; an empty Part Number shows the default.
fn standard_tab(ui: &mut egui::Ui, p: &mut IProperties, tab: Tab, file_name: &str) {
    egui::Grid::new(("ip_tab", tab as u8))
        .num_columns(2)
        .min_col_width(super::panels::label_width(ui.ctx()))
        .show(ui, |ui| {
            for (key, t, label) in STANDARD {
                if *t != tab {
                    continue;
                }
                super::panels::plabel(ui, label);
                let mut v = p.get(key).to_owned();
                let hint = if *key == "part_number" {
                    file_name.to_owned()
                } else if key.ends_with("_date") {
                    "YYYY-MM-DD".to_owned()
                } else {
                    String::new()
                };
                let multi = *key == "comments";
                let edit = if multi {
                    egui::TextEdit::multiline(&mut v).desired_rows(3)
                } else {
                    egui::TextEdit::singleline(&mut v)
                };
                let r = ui.add(edit.hint_text(hint).desired_width(f32::INFINITY));
                if r.changed() {
                    p.set(key, &v);
                }
                if key.ends_with("_date") && !v.is_empty() && !iprops::is_date(&v) {
                    r.on_hover_text("A date: YYYY-MM-DD");
                }
                ui.end_row();
            }
        });
}

fn custom_tab(ui: &mut egui::Ui, p: &mut IProperties, w: &mut IPropsWin) {
    let mut delete = None;
    egui::Grid::new("ip_custom")
        .num_columns(4)
        .striped(true)
        .show(ui, |ui| {
            ui.label(RichText::new("Name").strong());
            ui.label(RichText::new("Type").strong());
            ui.label(RichText::new("Value").strong());
            ui.label("");
            ui.end_row();
            for (k, c) in p.custom.iter_mut().enumerate() {
                ui.label(&c.name);
                ui.label(c.value.type_label());
                match &mut c.value {
                    PropValue::Text(s) => {
                        ui.text_edit_singleline(s);
                    }
                    PropValue::Date(s) => {
                        ui.add(egui::TextEdit::singleline(s).hint_text("YYYY-MM-DD"));
                    }
                    PropValue::Number(v) => {
                        ui.add(ExprValue::new(v));
                    }
                    PropValue::YesNo(b) => {
                        let label = if *b { "Yes" } else { "No" };
                        ui.checkbox(b, label);
                    }
                }
                if ui.small_button("x").on_hover_text("Delete").clicked() {
                    delete = Some(k);
                }
                ui.end_row();
            }
        });
    if let Some(k) = delete {
        p.custom.remove(k);
    }
    ui.separator();
    ui.label(RichText::new("New property").strong());
    ui.horizontal(|ui| {
        super::panels::plabel(ui, "Name");
        ui.text_edit_singleline(&mut w.new_name);
    });
    ui.horizontal(|ui| {
        super::panels::plabel(ui, "Type");
        egui::ComboBox::from_id_salt("ip_new_type")
            .selected_text(TYPES[w.new_type])
            .show_ui(ui, |ui| {
                for (k, t) in TYPES.iter().enumerate() {
                    ui.selectable_value(&mut w.new_type, k, *t);
                }
            });
    });
    ui.horizontal(|ui| {
        super::panels::plabel(ui, "Value");
        match w.new_type {
            0 => {
                ui.text_edit_singleline(&mut w.new_text);
            }
            1 => {
                ui.add(egui::TextEdit::singleline(&mut w.new_text).hint_text(iprops::today()));
            }
            2 => {
                ui.add(ExprValue::new(&mut w.new_number));
            }
            _ => {
                let label = if w.new_yes { "Yes" } else { "No" };
                ui.checkbox(&mut w.new_yes, label);
            }
        }
    });
    if ui.button("Add").clicked() {
        let value = match w.new_type {
            0 => Ok(PropValue::Text(w.new_text.clone())),
            1 => {
                let d = if w.new_text.trim().is_empty() {
                    iprops::today()
                } else {
                    w.new_text.trim().to_owned()
                };
                if iprops::is_date(&d) {
                    Ok(PropValue::Date(d))
                } else {
                    Err("a date is written YYYY-MM-DD".to_owned())
                }
            }
            2 => Ok(PropValue::Number(w.new_number)),
            _ => Ok(PropValue::YesNo(w.new_yes)),
        };
        w.error = match value.and_then(|v| p.add_custom(&w.new_name, v)) {
            Ok(()) => {
                w.new_name.clear();
                w.new_text.clear();
                None
            }
            Err(e) => Some(e),
        };
    }
}
