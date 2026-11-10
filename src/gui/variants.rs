//! The Variants window (`docs/spec/3.9-representations.md` §3.3a; Inventor: Model
//! States), for parts and assemblies alike: one row per item, one column per
//! variant, the active column marked, what differs from `[Primary]` tinted, and
//! the corrections (fill a row, copy a column, bulk over checked rows, CSV) as
//! lists of [`Set`]. The browser's folder stays the plain way to the same data.

use std::collections::BTreeSet;

use eframe::egui::{self, Color32, RichText, Stroke};

use super::CadCraftApp;
use super::theme;
use super::valuefield::{self, FieldSpec};
use crate::assembly::reps::{apply_sets, assembly_table, check_sets};
use crate::document::DocRef;
use crate::part::variants::{Cell, PRIMARY, Row, RowKey, Set, Table};

/// How often a part's active variant is brought up to date (seconds): reading
/// its parameters is too much for every frame, and every command that reads the
/// variants syncs first.
const SYNC_EVERY: f64 = 0.25;

/// Width of a variant's column.
const COL_W: f32 = 118.0;
/// Width of the items' column.
const NAME_W: f32 = 210.0;

/// The window's state.
#[derive(Clone, Debug, Default)]
pub(super) struct VariantsUi {
    /// It is open.
    pub open: bool,
    search: String,
    /// Only the rows that differ between the variants.
    differences: bool,
    /// Folded sections, by title.
    folded: BTreeSet<String>,
    /// Checked rows, for the bulk commands.
    checked: BTreeSet<RowKey>,
    /// Renaming a column: id, text, focus asked for.
    rename: Option<(u32, String, bool)>,
    /// The CSV box is open, with its text.
    csv: Option<String>,
    /// The document the rest of this state belongs to.
    doc: Option<crate::document::DocumentId>,
    /// When a part's variants were last brought up to date (seconds).
    synced: f64,
    /// Copy CSV was pressed this frame.
    copied: bool,
}

/// What the window asks for, run after it is drawn.
#[derive(Clone, Debug)]
enum Ask {
    Sets(Vec<Set>),
    New,
    Duplicate(u32),
    Rename(u32, String),
    Delete(u32),
    Activate(u32),
    Scope,
}

/// The table's rows that the search and the Differences filter leave.
fn shown<'a>(rows: &'a [Row], search: &str, differences: bool) -> Vec<&'a Row> {
    let q = search.trim().to_lowercase();
    rows.iter()
        .filter(|r| q.is_empty() || r.label.to_lowercase().contains(&q))
        .filter(|r| !differences || Table::differs(r))
        .collect()
}

impl CadCraftApp {
    /// Opens the window (Manage › Variants Table, the browser's folder).
    pub(super) fn open_variants(&mut self) {
        self.asm.variants.open = true;
    }

    /// A new variant of the active document.
    pub(super) fn new_variant_cmd(&mut self) {
        self.variants_ask(Ask::New);
    }

    /// Keeps a part's active variant up to date, once a frame.
    pub(super) fn part_variants_tick(&mut self, now: f64) {
        if self.is_assembly()
            || self.is_drawing()
            || self.part().variants.is_empty()
            || now - self.asm.variants.synced < SYNC_EVERY
        {
            return;
        }
        self.asm.variants.synced = now;
        self.part_mut().variants_sync();
        self.variants_take_doc_info();
    }

    /// The occurrence places variant `chosen` of its part (`None`: the active one).
    fn set_occurrence_variant(&mut self, id: u32, chosen: Option<u32>) {
        let r = self.asm_try("Part Variant", move |a| {
            if let Some(o) = a.find_mut(id) {
                o.component.variant = chosen;
            }
        });
        if let Err(e) = r {
            self.file_status = Some(Err(format!("Variant: {e}")));
        }
        self.scene_dirty = true;
        self.asm_refresh_status();
    }

    /// The row of an occurrence's Properties; nothing for a part without variants.
    pub(super) fn occurrence_variant_row(&mut self, ui: &mut egui::Ui, id: u32, r: DocRef) {
        let list = self.part_variant_names(r);
        if list.is_empty() {
            return;
        }
        // A pattern's copy places what its source places.
        if self.assembly().is_some_and(|a| a.is_element(id)) {
            super::panels::plabel(ui, "Variant");
            let shown = r
                .variant
                .and_then(|v| list.iter().find(|x| x.0 == v))
                .map_or(crate::assembly::reps::PART_ACTIVE, |x| x.1.as_str());
            ui.label(shown)
                .on_hover_text("A pattern's copy takes the variant of its source");
            ui.end_row();
            return;
        }
        super::panels::plabel(ui, "Variant");
        let shown = r
            .variant
            .and_then(|v| list.iter().find(|x| x.0 == v))
            .map_or(crate::assembly::reps::PART_ACTIVE, |x| x.1.as_str())
            .to_owned();
        let mut pick = None;
        egui::ComboBox::from_id_salt("occurrence_variant")
            .selected_text(shown)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(r.variant.is_none(), crate::assembly::reps::PART_ACTIVE)
                    .clicked()
                {
                    pick = Some(None);
                }
                for (v, n) in &list {
                    if ui.selectable_label(r.variant == Some(*v), n).clicked() {
                        pick = Some(Some(*v));
                    }
                }
            });
        ui.end_row();
        if let Some(c) = pick {
            self.set_occurrence_variant(id, c);
        }
    }

    /// The same in an occurrence's menu (rule 17).
    pub(super) fn occurrence_variant_menu(&mut self, ui: &mut egui::Ui, id: u32) {
        // A pattern's copy places what its source places.
        if self.assembly().is_some_and(|a| a.is_element(id)) {
            return;
        }
        let Some(r) = self
            .assembly()
            .and_then(|a| a.find(id))
            .map(|o| o.component)
        else {
            return;
        };
        let list = self.part_variant_names(r);
        if list.is_empty() {
            return;
        }
        let mut pick = None;
        ui.menu_button("Variant", |ui| {
            if ui
                .selectable_label(r.variant.is_none(), crate::assembly::reps::PART_ACTIVE)
                .clicked()
            {
                pick = Some(None);
                ui.close();
            }
            for (v, n) in &list {
                if ui.selectable_label(r.variant == Some(*v), n).clicked() {
                    pick = Some(Some(*v));
                    ui.close();
                }
            }
        });
        if let Some(c) = pick {
            self.set_occurrence_variant(id, c);
        }
    }

    /// A component's own variants; empty when it has only its `[Primary]` (or is
    /// no part).
    pub(super) fn part_variant_names(&self, r: DocRef) -> Vec<(u32, String)> {
        self.with_part(r.base(), |p| {
            p.variants
                .list
                .iter()
                .map(|v| (v.id, column_title(v.id, &v.name)))
                .collect()
        })
        .unwrap_or_default()
    }

    /// The active variant's Properties go into the document.
    pub(super) fn variants_push_info(&mut self) {
        let info = if self.is_assembly() {
            self.assembly()
                .and_then(|a| a.reps.active_variant())
                .map(|v| v.info.clone())
        } else if self.is_drawing() {
            None
        } else {
            self.part().active_variant_info().cloned()
        };
        let id = self.part_id;
        if let (Some(i), Some(d)) = (info, self.project.document_mut(id)) {
            i.write(&mut d.properties);
        }
    }

    /// The active variant takes the document's Properties.
    pub(super) fn variants_take_doc_info(&mut self) {
        let Some(props) = self
            .project
            .document(self.part_id)
            .map(|d| d.properties.clone())
        else {
            return;
        };
        if self.is_assembly() {
            if let Some(a) = self.assembly_mut() {
                a.reps.take_info(&props);
            }
        } else if !self.is_drawing() {
            self.part_mut().variants_take_info(&props);
        }
    }

    /// The active document's table.
    fn variants_model(&self) -> Option<Table> {
        let props = self
            .project
            .document(self.part_id)
            .map(|d| d.properties.clone())
            .unwrap_or_default();
        if self.is_assembly() {
            let lookup = |r: DocRef| self.part_variant_names(r);
            return self.assembly().map(|a| assembly_table(a, &props, &lookup));
        }
        (!self.is_drawing()).then(|| self.part().variants_table(&props))
    }

    /// The browser's folder of a part (an assembly's is in `reps_tree`).
    pub(super) fn part_variants_folder(&mut self, ui: &mut egui::Ui) {
        let Some(t) = self.variants_model() else {
            return;
        };
        let mut ask = None;
        let mut open = false;
        super::panels::tree_fold(ui, ui.make_persistent_id("part_variants"), false)
            .show_header(ui, |ui| {
                super::panels::tree_icon(ui, super::icons::Icon::RepState);
                ui.label("Variants").context_menu(|ui| {
                    if ui.button("New Variant").clicked() {
                        ask = Some(Ask::New);
                        ui.close();
                    }
                    if ui.button("Edit Table...").clicked() {
                        open = true;
                        ui.close();
                    }
                });
                if ui.small_button("+").on_hover_text("New Variant").clicked() {
                    ask = Some(Ask::New);
                }
            })
            .body(|ui| {
                for (id, name) in &t.columns {
                    let active = *id == t.active;
                    ui.horizontal(|ui| {
                        ui.allocate_ui(egui::vec2(12.0, ui.available_height()), |ui| {
                            ui.set_min_width(12.0);
                            if active {
                                ui.label("✔");
                            }
                        });
                        let shown = column_title(*id, name);
                        let r = ui.selectable_label(false, shown);
                        if r.double_clicked() && !active {
                            ask = Some(Ask::Activate(*id));
                        }
                        r.context_menu(|ui| {
                            if ui
                                .add_enabled(!active, egui::Button::new("Activate"))
                                .clicked()
                            {
                                ask = Some(Ask::Activate(*id));
                                ui.close();
                            }
                            if ui.button("Duplicate").clicked() {
                                ask = Some(Ask::Duplicate(*id));
                                ui.close();
                            }
                            if ui.button("Edit Table...").clicked() {
                                open = true;
                                ui.close();
                            }
                            if *id != PRIMARY
                                && ui
                                    .add_enabled(!active, egui::Button::new("Delete"))
                                    .clicked()
                            {
                                ask = Some(Ask::Delete(*id));
                                ui.close();
                            }
                        });
                    });
                }
            });
        if open {
            self.open_variants();
        }
        if let Some(a) = ask {
            self.variants_ask(a);
        }
    }

    /// The window.
    pub(super) fn variants_window(&mut self, ctx: &egui::Context) {
        if !self.asm.variants.open {
            return;
        }
        let Some(t) = self.variants_model() else {
            return;
        };
        let mut ui_state = std::mem::take(&mut self.asm.variants);
        // Another document: checks, the search and a rename do not carry over.
        if ui_state.doc != Some(self.part_id) {
            ui_state = VariantsUi {
                open: true,
                doc: Some(self.part_id),
                synced: ui_state.synced,
                ..Default::default()
            };
        }
        let table_rows = self.part_table_for_cells();
        let mut open = true;
        let mut asks: Vec<Ask> = Vec::new();
        let win = egui::Window::new("Variants")
            .id(egui::Id::new("variants_window"))
            .open(&mut open)
            .resizable(true)
            .default_size([840.0, 460.0])
            .min_height(240.0);
        self.layout
            .place("variants", ctx.content_rect(), true, win)
            .show(ctx, |ui| {
                toolbar(ui, &t, &mut ui_state, &mut asks);
                ui.separator();
                bulk_bar(ui, &t, &mut ui_state, &mut asks);
                csv_box(ui, &t, &mut ui_state, &mut asks, &mut self.file_status);
                // The column heads stay in view while the rows scroll, and go sideways with them.
                egui::ScrollArea::horizontal()
                    .id_salt("variants_scroll_x")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        heading(ui, &t, &mut ui_state, &mut asks);
                        egui::ScrollArea::vertical()
                            .id_salt("variants_scroll")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                for (title, rows) in &t.sections {
                                    section(
                                        ui,
                                        &t,
                                        title,
                                        rows,
                                        &mut ui_state,
                                        &mut asks,
                                        &table_rows,
                                    );
                                }
                            });
                    });
            });
        if std::mem::take(&mut ui_state.copied) {
            self.file_status = Some(Ok("Variants: the table was copied as CSV".to_owned()));
        }
        ui_state.open = open;
        self.asm.variants = ui_state;
        for a in asks {
            self.variants_ask(a);
        }
    }

    /// The parameters a value cell evaluates against.
    fn part_table_for_cells(&self) -> Vec<crate::part::params::ParamRow> {
        if self.is_assembly() || self.is_drawing() {
            Vec::new()
        } else {
            self.part().parameter_table()
        }
    }

    /// Runs what the window, the browser or the ribbon asked for.
    fn variants_ask(&mut self, a: Ask) {
        // The new variant starts with what the document has now.
        self.variants_take_doc_info();
        let is_new = matches!(a, Ask::New);
        if self.is_assembly() {
            self.assembly_variants_ask(a);
        } else if self.asm.in_place.is_some() {
            self.file_status = Some(Err(
                "Variants: open the part itself to change them, not in place".to_owned(),
            ));
        } else if !self.is_drawing() {
            self.part_variants_ask(a);
        }
        if is_new {
            self.variants_fill_new_info();
        }
        self.variants_push_info();
    }

    /// A list that has just come to life has no Properties in its entries yet.
    fn variants_fill_new_info(&mut self) {
        let Some(props) = self
            .project
            .document(self.part_id)
            .map(|d| d.properties.clone())
        else {
            return;
        };
        if self.is_assembly() {
            if let Some(a) = self.assembly_mut() {
                a.reps.fill_info(&props);
            }
        } else if !self.is_drawing() {
            self.part_mut().variants_fill_info(&props);
        }
    }

    fn part_variants_ask(&mut self, a: Ask) {
        let label = match &a {
            Ask::Sets(_) => "Edit Variants",
            Ask::New => "New Variant",
            Ask::Duplicate(_) => "Duplicate Variant",
            Ask::Rename(..) => "Rename Variant",
            Ask::Delete(_) => "Delete Variant",
            Ask::Activate(_) => "Activate Variant",
            Ask::Scope => "Toggle Edit Scope",
        };
        let camera_dirty = matches!(a, Ask::Activate(_) | Ask::Duplicate(_) | Ask::Sets(_));
        self.edit_part_variants(label, move |p| {
            let map = |e: crate::part::PartError| e.to_string();
            match a {
                Ask::New => {
                    p.new_variant();
                }
                Ask::Duplicate(id) => {
                    p.duplicate_variant(id).map_err(map)?;
                }
                Ask::Activate(id) => p.activate_variant(id).map_err(map)?,
                Ask::Delete(id) => {
                    p.delete_variant(id);
                }
                Ask::Scope => p.variants.factory_scope = !p.variants.factory_scope,
                Ask::Rename(id, name) => {
                    let own = p.variants.get(id).map_or(String::new(), |v| v.name.clone());
                    if !p.variants.name_free(&name, &own) {
                        return Err(format!("the name {} is taken", name.trim()));
                    }
                    if let Some(v) = p.variants.list.iter_mut().find(|v| v.id == id) {
                        v.name = name.trim().to_owned();
                    }
                }
                Ask::Sets(sets) => {
                    for s in sets {
                        match (&s.key, &s.cell) {
                            (RowKey::Feature(f), Cell::Suppressed(on)) => {
                                p.set_variant_suppressed(s.col, f, *on).map_err(map)?;
                            }
                            (RowKey::Param(n), Cell::Value { text, .. }) => {
                                p.set_variant_param(s.col, n, text).map_err(map)?;
                            }
                            (key, cell) => {
                                p.set_variant_info(s.col, key, cell).map_err(map)?;
                            }
                        }
                    }
                }
            }
            p.variants_sync();
            Ok(true)
        });
        if camera_dirty {
            self.scene_dirty = true;
        }
    }

    fn assembly_variants_ask(&mut self, a: Ask) {
        use super::reps::RepKind;
        match a {
            Ask::New => self.new_representation(RepKind::Variant),
            Ask::Activate(id) => self.rep_activate(RepKind::Variant, id),
            Ask::Delete(id) => self.rep_delete(RepKind::Variant, id),
            Ask::Scope => self.rep_toggle_scope(),
            Ask::Rename(id, name) => self.rep_rename(RepKind::Variant, id, &name),
            Ask::Duplicate(id) => {
                let r = self.asm_try("Duplicate Variant", move |a| {
                    let mut r = std::mem::take(&mut a.reps);
                    r.duplicate_variant(id, a);
                    a.reps = r;
                });
                if let Err(e) = r {
                    self.file_status = Some(Err(format!("Variants: {e}")));
                }
            }
            Ask::Sets(sets) => {
                let checked = self.assembly().map_or(Ok(()), |a| check_sets(a, &sets));
                if let Err(e) = checked {
                    self.file_status = Some(Err(format!("Variants: {e}")));
                    return;
                }
                // The names are read first: the closure has the assembly.
                let names: std::collections::HashMap<DocRef, Vec<(u32, String)>> = self
                    .assembly()
                    .map(|a| {
                        a.occurrences
                            .iter()
                            .map(|o| o.component.base())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
                    .into_iter()
                    .map(|r| (r, self.part_variant_names(r)))
                    .collect();
                let lookup = move |r: DocRef| names.get(&r).cloned().unwrap_or_default();
                let r = self.asm_try("Edit Variants", move |a| apply_sets(a, &sets, &lookup));
                if let Err(e) = r {
                    self.file_status = Some(Err(format!("Variants: {e}")));
                }
                self.asm_refresh_status();
            }
        }
    }

    /// One Undo step on a copy, like the part's other edits (rule 5).
    fn edit_part_variants(
        &mut self,
        label: &str,
        f: impl FnOnce(&mut crate::part::PartDocument) -> Result<bool, String>,
    ) {
        let mut trial = self.part().clone();
        match f(&mut trial) {
            Ok(_) => {
                self.history.break_coalescing();
                self.checkpoint(label);
                *self.part_mut() = trial;
                self.history.break_coalescing();
                self.scene_dirty = true;
                self.file_status = None;
            }
            Err(e) => self.file_status = Some(Err(format!("Variants: {e}"))),
        }
    }
}

fn column_title(id: u32, name: &str) -> String {
    if id == PRIMARY {
        "[Primary]".to_owned()
    } else {
        name.to_owned()
    }
}

fn toolbar(ui: &mut egui::Ui, t: &Table, st: &mut VariantsUi, asks: &mut Vec<Ask>) {
    ui.horizontal_wrapped(|ui| {
        if ui
            .add(theme::ok_button("New Variant"))
            .on_hover_text("A new variant from the document as it is now; it becomes active")
            .clicked()
        {
            asks.push(Ask::New);
        }
        if ui
            .button("Duplicate")
            .on_hover_text("A copy of the active variant")
            .clicked()
        {
            asks.push(Ask::Duplicate(t.active));
        }
        ui.separator();
        ui.add(
            egui::TextEdit::singleline(&mut st.search)
                .hint_text("Search items")
                .desired_width(150.0),
        );
        ui.checkbox(&mut st.differences, "Only differences")
            .on_hover_text("Only the rows that are not the same in every variant");
        ui.separator();
        let (text, tip) = if t.factory_scope {
            (
                "Edit Scope: Factory",
                "Changes made in the model go to every variant; click for the active one only",
            )
        } else {
            (
                "Edit Scope: Member",
                "Changes made in the model go to the active variant only; click for all of them",
            )
        };
        if ui.button(text).on_hover_text(tip).clicked() {
            asks.push(Ask::Scope);
        }
        ui.separator();
        if ui
            .button("Copy CSV")
            .on_hover_text("The table as text, to paste into a spreadsheet")
            .clicked()
        {
            ui.ctx().copy_text(t.to_csv());
            st.copied = true;
        }
        if ui
            .selectable_label(st.csv.is_some(), "Paste CSV...")
            .on_hover_text("Bring an edited table back from a spreadsheet")
            .clicked()
        {
            st.csv = if st.csv.is_some() {
                None
            } else {
                Some(String::new())
            };
        }
    });
}

/// The commands for the checked rows: always there, so that checking the first
/// row does not move the rows under the cursor.
fn bulk_bar(ui: &mut egui::Ui, t: &Table, st: &mut VariantsUi, asks: &mut Vec<Ask>) {
    let keys: Vec<RowKey> = st.checked.iter().cloned().collect();
    let active = column_title(
        t.active,
        t.columns
            .iter()
            .find(|c| c.0 == t.active)
            .map_or("", |c| c.1.as_str()),
    );
    let any = !keys.is_empty();
    ui.horizontal_wrapped(|ui| {
        ui.label(if any {
            RichText::new(format!("{} checked:", keys.len())).strong()
        } else {
            RichText::new("Checked rows:").weak()
        });
        ui.add_enabled_ui(any, |ui| {
            if ui.button(format!("Suppress in {active}")).clicked() {
                asks.push(Ask::Sets(t.set_suppressed(&keys, t.active, true)));
            }
            if ui.button(format!("Include in {active}")).clicked() {
                asks.push(Ask::Sets(t.set_suppressed(&keys, t.active, false)));
            }
            if ui
                .button("Same in all variants")
                .on_hover_text("Every variant takes the active variant's setting of these rows")
                .clicked()
            {
                let mut sets = Vec::new();
                for k in &keys {
                    sets.extend(t.fill_row(k, t.active));
                }
                asks.push(Ask::Sets(sets));
            }
            if ui
                .add_enabled(t.active != PRIMARY, egui::Button::new("Back to [Primary]"))
                .on_hover_text("The active variant takes [Primary]'s setting of these rows")
                .clicked()
            {
                asks.push(Ask::Sets(t.copy_column(PRIMARY, t.active, Some(&keys))));
            }
            if ui.button("Clear").clicked() {
                st.checked.clear();
            }
        });
    });
    ui.separator();
}

fn csv_box(
    ui: &mut egui::Ui,
    t: &Table,
    st: &mut VariantsUi,
    asks: &mut Vec<Ask>,
    status: &mut Option<Result<String, String>>,
) {
    let Some(text) = &mut st.csv else {
        return;
    };
    ui.add(
        egui::TextEdit::multiline(text)
            .hint_text("Section,Item,[variant names]")
            .desired_rows(4)
            .desired_width(f32::INFINITY),
    );
    let mut close = false;
    ui.horizontal(|ui| {
        if ui.button("Apply").clicked() {
            match t.from_csv(text) {
                Ok(sets) => {
                    *status = Some(Ok(format!("Variants: cells changed: {}", sets.len())));
                    asks.push(Ask::Sets(sets));
                    close = true;
                }
                Err(e) => *status = Some(Err(format!("Variants: {e}"))),
            }
        }
        if ui.button("Cancel").clicked() {
            close = true;
        }
    });
    ui.separator();
    if close {
        st.csv = None;
    }
}

/// The row of variant names.
fn heading(ui: &mut egui::Ui, t: &Table, st: &mut VariantsUi, asks: &mut Vec<Ask>) {
    let tk = theme::tokens();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.add_sized(
            [NAME_W, 26.0],
            egui::Label::new(RichText::new("Item").strong()),
        );
        for (id, name) in &t.columns {
            let active = *id == t.active;
            if let Some((rid, text, focused)) = &mut st.rename
                && *rid == *id
            {
                let e = ui.add_sized([COL_W, 24.0], egui::TextEdit::singleline(text));
                if !*focused {
                    valuefield::focus_all(ui.ctx(), &e, text);
                    *focused = true;
                }
                if e.lost_focus() {
                    let keep = ui.input(|i| !i.key_pressed(egui::Key::Escape));
                    let new = text.clone();
                    st.rename = None;
                    if keep && new.trim() != name {
                        asks.push(Ask::Rename(*id, new));
                    }
                }
                continue;
            }
            let title = column_title(*id, name);
            let (rect, r) = ui.allocate_exact_size(egui::vec2(COL_W, 26.0), egui::Sense::click());
            let fill = if active { tk.accent } else { tk.control };
            ui.painter().rect_filled(rect, 4.0, fill);
            let color = if active { tk.on_accent } else { tk.text };
            let shown = if active {
                format!("✔ {title}")
            } else {
                title
            };
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                shown,
                egui::FontId::proportional(13.0),
                color,
            );
            let r = r.on_hover_text(if active {
                "The active variant; right-click for its commands"
            } else {
                "Double-click to activate; right-click for its commands"
            });
            if r.double_clicked() && !active {
                asks.push(Ask::Activate(*id));
            }
            r.context_menu(|ui| {
                if ui
                    .add_enabled(!active, egui::Button::new("Activate"))
                    .clicked()
                {
                    asks.push(Ask::Activate(*id));
                    ui.close();
                }
                if ui.button("Duplicate").clicked() {
                    asks.push(Ask::Duplicate(*id));
                    ui.close();
                }
                if *id != PRIMARY {
                    if ui.button("Rename").clicked() {
                        st.rename = Some((*id, name.clone(), false));
                        ui.close();
                    }
                    if ui
                        .button("Same as [Primary]")
                        .on_hover_text("All its settings go back to [Primary]'s")
                        .clicked()
                    {
                        asks.push(Ask::Sets(t.copy_column(PRIMARY, *id, None)));
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .add_enabled(!active, egui::Button::new("Delete"))
                        .clicked()
                    {
                        asks.push(Ask::Delete(*id));
                        ui.close();
                    }
                }
            });
        }
    });
    ui.add_space(2.0);
}

#[allow(clippy::too_many_arguments)]
fn section(
    ui: &mut egui::Ui,
    t: &Table,
    title: &str,
    rows: &[Row],
    st: &mut VariantsUi,
    asks: &mut Vec<Ask>,
    params: &[crate::part::params::ParamRow],
) {
    let tk = theme::tokens();
    let rows = shown(rows, &st.search, st.differences);
    // A filter leaves only the sections it found something in.
    if rows.is_empty() && (st.differences || !st.search.trim().is_empty()) {
        return;
    }
    let folded = st.folded.contains(title);
    let n = rows.iter().filter(|r| Table::differs(r)).count();
    let head = format!(
        "{} {title}  ({}{})",
        if folded { ">" } else { "v" },
        rows.len(),
        if n > 0 {
            format!(", {n} differ")
        } else {
            String::new()
        }
    );
    let r = ui.add(
        egui::Label::new(RichText::new(head).strong().color(tk.text_strong))
            .sense(egui::Sense::click()),
    );
    if r.clicked() {
        if folded {
            st.folded.remove(title);
        } else {
            st.folded.insert(title.to_owned());
        }
    }
    if folded {
        ui.add_space(4.0);
        return;
    }
    for row in rows.iter() {
        let resp = ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.allocate_ui_with_layout(
                egui::vec2(NAME_W, 24.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_width(NAME_W);
                    let mut on = st.checked.contains(&row.key);
                    if ui.checkbox(&mut on, "").changed() {
                        if on {
                            st.checked.insert(row.key.clone());
                        } else {
                            st.checked.remove(&row.key);
                        }
                    }
                    let name = ui.add(
                        egui::Label::new(&row.label)
                            .truncate()
                            .sense(egui::Sense::click()),
                    );
                    name.context_menu(|ui| {
                        if ui
                            .button("Same in all variants")
                            .on_hover_text("Every variant takes the active variant's setting")
                            .clicked()
                        {
                            asks.push(Ask::Sets(t.fill_row(&row.key, t.active)));
                            ui.close();
                        }
                        if t.active != PRIMARY && ui.button("Back to [Primary]").clicked() {
                            asks.push(Ask::Sets(t.copy_column(
                                PRIMARY,
                                t.active,
                                Some(std::slice::from_ref(&row.key)),
                            )));
                            ui.close();
                        }
                    });
                },
            );
            for (c, (id, _)) in t.columns.iter().enumerate() {
                cell(ui, t, row, c, *id, asks, params);
            }
        });
        // A quiet line under each row keeps the eye on it.
        let y = resp.response.rect.bottom() + 1.0;
        ui.painter().hline(
            resp.response.rect.left()..=resp.response.rect.right(),
            y,
            Stroke::new(1.0, tk.line.gamma_multiply(0.4)),
        );
        ui.add_space(2.0);
    }
    ui.add_space(6.0);
}

/// One cell: a pill to click for a suppression, the usual field for a value.
fn cell(
    ui: &mut egui::Ui,
    t: &Table,
    row: &Row,
    c: usize,
    id: u32,
    asks: &mut Vec<Ask>,
    params: &[crate::part::params::ParamRow],
) {
    let tk = theme::tokens();
    let differs = c > 0 && !row.cells[c].same(&row.cells[0]);
    match &row.cells[c] {
        Cell::Suppressed(on) => {
            let (rect, r) = ui.allocate_exact_size(egui::vec2(COL_W, 24.0), egui::Sense::click());
            let (fill, ink, word) = pill_look(*on, differs, r.hovered(), id == t.active);
            ui.painter()
                .rect_filled(rect.shrink2(egui::vec2(3.0, 2.0)), 10.0, fill);
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                word,
                egui::FontId::proportional(12.0),
                ink,
            );
            if r.on_hover_text(if *on {
                "Suppressed here: click to include"
            } else {
                "Included here: click to suppress"
            })
            .clicked()
            {
                asks.push(Ask::Sets(vec![Set {
                    key: row.key.clone(),
                    col: id,
                    cell: Cell::Suppressed(!*on),
                }]));
            }
        }
        Cell::Text(text) => {
            let cell_id = egui::Id::new(("variant_text", &row.key, id));
            let mut buf: String = ui
                .data(|d| d.get_temp(cell_id))
                .unwrap_or_else(|| text.clone());
            let e = ui.add_sized(
                [COL_W, 22.0],
                egui::TextEdit::singleline(&mut buf).text_color(if differs {
                    tk.warning
                } else {
                    tk.text
                }),
            );
            if e.has_focus() {
                ui.data_mut(|d| d.insert_temp(cell_id, buf.clone()));
            }
            if e.lost_focus() {
                ui.data_mut(|d| d.remove::<String>(cell_id));
                let keep = ui.input(|i| !i.key_pressed(egui::Key::Escape));
                if keep && buf.trim() != text {
                    asks.push(Ask::Sets(vec![Set {
                        key: row.key.clone(),
                        col: id,
                        cell: Cell::Text(buf),
                    }]));
                }
            }
        }
        Cell::Choice { options, selected } => {
            let frame = egui::Frame::new()
                .stroke(if differs {
                    Stroke::new(1.5, tk.warning)
                } else {
                    Stroke::NONE
                })
                .corner_radius(4.0);
            frame.show(ui, |ui| {
                ui.set_width(COL_W);
                egui::ComboBox::from_id_salt(("variant_choice", &row.key, id))
                    .selected_text(selected.as_str())
                    .width(COL_W - 4.0)
                    .show_ui(ui, |ui| {
                        for o in options {
                            if ui.selectable_label(o == selected, o).clicked() && o != selected {
                                asks.push(Ask::Sets(vec![Set {
                                    key: row.key.clone(),
                                    col: id,
                                    cell: Cell::Choice {
                                        options: options.clone(),
                                        selected: o.clone(),
                                    },
                                }]));
                            }
                        }
                    });
            });
        }
        Cell::Value { text, unit, value } => {
            let cell_id = egui::Id::new(("variant_cell", &row.key, id));
            // The field paints over a fill, so what differs gets a frame.
            let frame = egui::Frame::new()
                .stroke(if differs {
                    Stroke::new(1.5, tk.warning)
                } else {
                    Stroke::NONE
                })
                .corner_radius(4.0);
            frame.show(ui, |ui| {
                ui.set_width(COL_W);
                ui.spacing_mut().interact_size.x = 54.0;
                let spec = FieldSpec {
                    id: cell_id,
                    value: *value,
                    unit: *unit,
                    name: None,
                    expr: Some(text),
                    table: params,
                    quantity: None,
                };
                if let Some(new) = valuefield::value_field(ui, &spec)
                    && new.trim() != text
                {
                    asks.push(Ask::Sets(vec![Set {
                        key: row.key.clone(),
                        col: id,
                        cell: Cell::Value {
                            text: new,
                            unit: *unit,
                            value: *value,
                        },
                    }]));
                }
            });
        }
    }
}

/// (fill, text colour, word) of a suppression pill.
fn pill_look(
    on: bool,
    differs: bool,
    hovered: bool,
    active: bool,
) -> (Color32, Color32, &'static str) {
    let tk = theme::tokens();
    let (base, word) = if on {
        (tk.error, "Suppressed")
    } else {
        (tk.ok, "Included")
    };
    let strength: f32 = match (differs, active) {
        (true, _) => 0.42,
        (false, true) => 0.2,
        (false, false) => 0.12,
    };
    let strength = if hovered { strength + 0.15 } else { strength };
    let ink = if differs { tk.text_strong } else { tk.text };
    (base.gamma_multiply(strength), ink, word)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_filters_keep_what_is_asked_for() {
        let row = |label: &str, a: bool, b: bool| Row {
            key: RowKey::Occurrence(1),
            label: label.to_owned(),
            cells: vec![Cell::Suppressed(a), Cell::Suppressed(b)],
        };
        let rows = [row("Bolt:1", false, true), row("Plate:1", false, false)];
        assert_eq!(shown(&rows, "", false).len(), 2);
        assert_eq!(shown(&rows, "", true).len(), 1);
        assert_eq!(shown(&rows, "plate", false).len(), 1);
        assert!(shown(&rows, "plate", true).is_empty());
    }
}
