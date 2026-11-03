//! Manage › Author › Create Part Family (`docs/spec/5.7-part-families.md`):
//! the table of the active part, and Publish into the Component Library.
//! Nothing changes until OK (one Undo step); cells are expressions over the
//! row's columns, so they are text cells rather than value fields.

use eframe::egui::{self, Color32, RichText};

use super::CadCraftApp;
use crate::part::expr::Unit;
use crate::part::family::{Column, FamilyTable, Issue, Target};

/// The window's state.
#[derive(Default)]
pub(super) struct PartFamilyUi {
    table: FamilyTable,
    issues: Vec<Issue>,
    /// Text for Import CSV.
    csv: String,
    show_csv: bool,
    category: String,
    name: String,
    message: Option<Result<String, String>>,
}

/// A number as a cell shows it.
fn cell_number(v: f64) -> String {
    let s = format!("{v:.6}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

impl PartFamilyUi {
    fn add_column(&mut self, mut c: Column, first_value: String) {
        let base = c.name.clone();
        let mut k = 2;
        while self.table.columns.iter().any(|o| o.name == c.name) {
            c.name = format!("{base}_{k}");
            k += 1;
        }
        self.table.columns.push(c);
        if self.table.rows.is_empty() {
            self.table.rows.push(Vec::new());
        }
        for r in &mut self.table.rows {
            r.push(first_value.clone());
        }
    }
}

impl CadCraftApp {
    /// Opens the window on the part's table, or a new one.
    pub(super) fn part_family_start(&mut self) {
        let table = self.part().family.clone().unwrap_or_else(|| FamilyTable {
            id: uuid::Uuid::new_v4().to_string(),
            ..Default::default()
        });
        self.asm.part_family = Some(PartFamilyUi {
            table,
            category: "Own Parts".into(),
            name: self
                .project
                .document(self.part_id)
                .map(|d| d.name.clone())
                .unwrap_or_default(),
            ..Default::default()
        });
    }

    /// The window.
    pub(super) fn part_family_window(&mut self, ctx: &egui::Context) {
        let Some(mut st) = self.asm.part_family.take() else {
            return;
        };
        let params = self.part().parameter_table();
        let features: Vec<(String, String)> = self
            .part()
            .features
            .iter()
            .map(|f| (f.id().to_owned(), f.name().to_owned()))
            .collect();
        let mut open = true;
        let (mut ok, mut cancel, mut verify, mut publish) = (false, false, false, false);
        let w = egui::Window::new("Create Part Family")
            .open(&mut open)
            .resizable(true)
            .collapsible(false)
            .default_size([980.0, 560.0])
            .min_size([640.0, 360.0]);
        self.layout.place("part_family", ctx.content_rect(), true, w).show(ctx, |ui| {
            egui::Panel::bottom("part_family_buttons").show(ui, |ui| {
                for i in st.issues.iter().take(6) {
                    let at = match (i.row, i.column) {
                        (Some(r), _) => format!("Member {}: ", r + 1),
                        (None, Some(c)) => st
                            .table
                            .columns
                            .get(c)
                            .map(|x| format!("{}: ", x.heading()))
                            .unwrap_or_default(),
                        _ => String::new(),
                    };
                    ui.colored_label(
                        Color32::from_rgb(235, 80, 70),
                        format!("{at}{}", i.message),
                    );
                }
                match &st.message {
                    Some(Ok(t)) => {
                        ui.colored_label(super::theme::tokens().ok, t);
                    }
                    Some(Err(t)) => {
                        ui.colored_label(super::theme::tokens().error, t);
                    }
                    None => {}
                }
                ui.horizontal(|ui| {
                    ui.label("Category");
                    ui.add(egui::TextEdit::singleline(&mut st.category).desired_width(200.0))
                        .on_hover_text("Folders in the library, separated by /");
                    ui.label("Family");
                    ui.add(egui::TextEdit::singleline(&mut st.name).desired_width(180.0));
                    publish = ui
                        .button("Publish")
                        .on_hover_text("Saves this part with its table into your library; Place from Library then lists it")
                        .clicked();
                });
                ui.horizontal(|ui| {
                    ok = ui.add(super::theme::ok_button("OK")).clicked();
                    cancel = ui.button("Cancel").clicked();
                    verify = ui.button("Verify").clicked();
                    if ui.button("Copy as CSV").clicked() {
                        ui.ctx().copy_text(st.table.to_csv());
                    }
                    if ui.selectable_label(st.show_csv, "Paste CSV...").clicked() {
                        st.show_csv = !st.show_csv;
                    }
                });
            });
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(220.0);
                    egui::ScrollArea::vertical().id_salt("pf_sources").show(ui, |ui| {
                        ui.label(RichText::new("Parameters").strong());
                        for p in params.iter().filter(|p| p.unit != Unit::Text) {
                            let used = st.table.columns.iter().any(
                                |c| matches!(&c.target, Target::Parameter(n) if *n == p.name),
                            );
                            let text = format!(
                                ">> {} = {} {}",
                                p.name,
                                cell_number(p.unit.to_display(p.value)),
                                p.unit.label()
                            );
                            if ui.add_enabled(!used, egui::Button::new(text)).clicked() {
                                st.add_column(
                                    Column::new(
                                        &p.name,
                                        p.unit,
                                        Target::Parameter(p.name.clone()),
                                    ),
                                    cell_number(p.unit.to_display(p.value)),
                                );
                            }
                        }
                        ui.separator();
                        ui.label(RichText::new("Properties").strong());
                        for (key, label) in [
                            ("part_number", "Part Number"),
                            ("description", "Description"),
                            ("stock_number", "Stock Number"),
                        ] {
                            let target = Target::Property(key.to_owned());
                            let used = st.table.columns.iter().any(|c| c.target == target);
                            if ui
                                .add_enabled(!used, egui::Button::new(format!(">> {label}")))
                                .clicked()
                            {
                                let name = label.replace(' ', "_");
                                st.add_column(Column::new(&name, Unit::Text, target), String::new());
                            }
                        }
                        let used = st.table.columns.iter().any(|c| c.target == Target::Material);
                        if ui.add_enabled(!used, egui::Button::new(">> Material")).clicked() {
                            let m = self.part().material.clone().unwrap_or_default();
                            st.add_column(
                                Column::new("Material", Unit::Text, Target::Material),
                                m,
                            );
                        }
                        let used = st.table.columns.iter().any(|c| c.target == Target::FileName);
                        if ui.add_enabled(!used, egui::Button::new(">> File Name")).clicked() {
                            st.add_column(
                                Column::new("File_Name", Unit::Text, Target::FileName),
                                String::new(),
                            );
                        }
                        ui.separator();
                        ui.label(RichText::new("Suppression").strong());
                        for (id, name) in &features {
                            let target = Target::Suppress(id.clone());
                            let used = st.table.columns.iter().any(|c| c.target == target);
                            if ui
                                .add_enabled(!used, egui::Button::new(format!(">> {name}")))
                                .on_hover_text("true in a cell leaves this feature out of that member")
                                .clicked()
                            {
                                let name: String = name
                                    .chars()
                                    .map(|c| if c.is_alphanumeric() { c } else { '_' })
                                    .collect();
                                st.add_column(
                                    Column::new(&format!("No_{name}"), Unit::Bool, target),
                                    "false".into(),
                                );
                            }
                        }
                    });
                });
                ui.separator();
                ui.vertical(|ui| {
                    if st.show_csv {
                        ui.add(
                            egui::TextEdit::multiline(&mut st.csv)
                                .desired_rows(4)
                                .desired_width(f32::INFINITY)
                                .hint_text("Paste rows here, headed by the column names"),
                        );
                        if ui.button("Read").clicked() {
                            st.message = Some(
                                st.table
                                    .read_csv(&st.csv.clone())
                                    .map(|n| format!("{n} members read")),
                            );
                        }
                        ui.separator();
                    }
                    table_grid(ui, &mut st.table);
                });
            });
        });
        if verify {
            st.issues = st.table.verify(self.part());
            st.message = st
                .issues
                .is_empty()
                .then(|| Ok("The table is fine".to_owned()));
        }
        if publish {
            st.issues = st.table.verify(self.part());
            if st.issues.is_empty() {
                let mut part = self.part().clone();
                part.family = Some(st.table.clone());
                let props = self
                    .project
                    .document(self.part_id)
                    .map(|d| d.properties.clone())
                    .unwrap_or_default();
                st.message = Some(match crate::components::root_dir() {
                    Some(root) => crate::components::catalog::publish(
                        &root,
                        &st.category,
                        &st.name,
                        &part,
                        &props,
                    )
                    .map(|p| format!("Published as {}", p.display())),
                    None => Err("no user folder for the library".to_owned()),
                });
            }
        }
        if ok {
            st.issues = if st.table.columns.is_empty() {
                Vec::new()
            } else {
                st.table.verify(self.part())
            };
            if st.issues.is_empty() {
                self.checkpoint("Create Part Family");
                self.part_mut().family = (!st.table.columns.is_empty()).then(|| st.table.clone());
                self.history.break_coalescing();
                return;
            }
        }
        if !(cancel || !open) {
            self.asm.part_family = Some(st);
        }
    }
}

/// The table: a heading per column (Key, delete), a row per member.
fn table_grid(ui: &mut egui::Ui, t: &mut FamilyTable) {
    if t.columns.is_empty() {
        ui.weak("Add columns with the >> buttons on the left.");
        return;
    }
    let mut drop_column = None;
    let mut drop_row = None;
    let mut toggle_key = None;
    egui::ScrollArea::both().id_salt("pf_table").show(ui, |ui| {
        egui::Grid::new("pf_grid").striped(true).show(ui, |ui| {
            ui.label("");
            for (i, c) in t.columns.iter().enumerate() {
                ui.vertical(|ui| {
                    ui.label(RichText::new(c.heading()).strong());
                    ui.horizontal(|ui| {
                        let text = if c.key > 0 {
                            format!("Key {}", c.key)
                        } else {
                            "Key".to_owned()
                        };
                        if ui
                            .selectable_label(c.key > 0, text)
                            .on_hover_text("Key columns choose the member, in this order")
                            .clicked()
                        {
                            toggle_key = Some(i);
                        }
                        if super::theme::glyph(ui, super::theme::Glyph::Close)
                            .on_hover_text("Delete the column")
                            .clicked()
                        {
                            drop_column = Some(i);
                        }
                    });
                    if !c.unit.label().is_empty() && c.unit != Unit::Text {
                        ui.weak(c.unit.label());
                    }
                });
            }
            ui.end_row();
            let n = t.rows.len();
            for r in 0..n {
                ui.horizontal(|ui| {
                    if super::theme::glyph(ui, super::theme::Glyph::Close)
                        .on_hover_text("Delete the member")
                        .clicked()
                    {
                        drop_row = Some(r);
                    }
                    let mark = if t.default_row == r { "*" } else { " " };
                    if ui
                        .small_button(format!("{}{mark}", r + 1))
                        .on_hover_text("Set as the default member (*)")
                        .clicked()
                    {
                        t.default_row = r;
                    }
                });
                for k in 0..t.columns.len() {
                    if let Some(cell) = t.rows[r].get_mut(k) {
                        ui.add(egui::TextEdit::singleline(cell).desired_width(84.0));
                    }
                }
                ui.end_row();
            }
        });
        if ui.button("Add Member").clicked() {
            let copy = t
                .rows
                .last()
                .cloned()
                .unwrap_or_else(|| vec![String::new(); t.columns.len()]);
            t.rows.push(copy);
        }
    });
    if let Some(i) = toggle_key {
        let next = t.columns.iter().map(|c| c.key).max().unwrap_or(0) + 1;
        let c = &mut t.columns[i];
        c.key = if c.key > 0 { 0 } else { next };
    }
    if let Some(i) = drop_column {
        t.columns.remove(i);
        for r in &mut t.rows {
            if i < r.len() {
                r.remove(i);
            }
        }
    }
    if let Some(r) = drop_row {
        t.rows.remove(r);
        t.default_row = t.default_row.min(t.rows.len().saturating_sub(1));
    }
}
