//! The explanations of the calculating generators in the interface (spec
//! 6.2-design-calculators §3.6, rule 0-ui §4.3): a **?** beside each field
//! (hover: its entry; click: it goes to the pane), a hover on each result
//! with its formula and the formula with the numbers in it, and the
//! Explanations pane. The texts are `design::explain`'s.

use eframe::egui::{self, RichText};

use crate::design::explain::{self, Entry, Formula};

/// The pane's current entry: (generator, id, is a result).
fn current_id(generator: &str) -> egui::Id {
    egui::Id::new(("explain_current", generator.to_owned()))
}

/// The entry `e` as hover / pane text.
fn entry_ui(ui: &mut egui::Ui, e: &Entry, formula: Option<&Formula>, numbers: Option<&str>) {
    ui.set_max_width(380.0);
    let mut head = super::i18n::tr(e.id).into_owned();
    if !e.symbol.is_empty() || !e.unit.is_empty() {
        head.push_str(&format!(
            "  ({}{}{})",
            e.symbol,
            if e.symbol.is_empty() || e.unit.is_empty() {
                ""
            } else {
                ", "
            },
            e.unit
        ));
    }
    ui.label(RichText::new(head).strong());
    ui.add(egui::Label::new(super::i18n::tr(e.what)).wrap());
    if e.typical != "—" {
        ui.add(
            egui::Label::new(format!(
                "{}: {}",
                super::i18n::tr("Typical"),
                super::i18n::tr(e.typical)
            ))
            .wrap(),
        );
    }
    for t in [e.low, e.high] {
        if t != "—" {
            ui.add(egui::Label::new(RichText::new(super::i18n::tr(t)).weak()).wrap());
        }
    }
    if let Some(f) = formula {
        formula_ui(ui, f);
    }
    if let Some(n) = numbers {
        ui.add(egui::Label::new(RichText::new(n).monospace()).wrap());
    }
}

fn formula_ui(ui: &mut egui::Ui, f: &Formula) {
    ui.add(egui::Label::new(RichText::new(f.formula).monospace()).wrap());
    if f.symbols != "—" {
        ui.add(egui::Label::new(RichText::new(super::i18n::tr(f.symbols)).small()).wrap());
    }
    ui.add(
        egui::Label::new(
            RichText::new(format!("{} {}", f.source, super::i18n::tr(f.limits)))
                .small()
                .weak(),
        )
        .wrap(),
    );
}

/// A Properties label cell (as `panels::plabel`) with a **?** after the text
/// when `generator` explains `label`: hover shows the entry, a click puts it
/// in the Explanations pane.
pub(super) fn plabel_help(ui: &mut egui::Ui, label: &str, generator: Option<&str>) {
    let Some((g, e)) = generator.and_then(|g| Some((g, explain::input(g, label)?))) else {
        super::panels::plabel(ui, label);
        return;
    };
    let w = super::panels::label_width(ui.ctx());
    let color = ui.visuals().weak_text_color();
    ui.allocate_ui_with_layout(
        egui::vec2(w, 18.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_width(w);
            ui.set_max_width(w);
            ui.allocate_ui(egui::vec2(w - 16.0, 18.0), |ui| {
                ui.add(egui::Label::new(RichText::new(label).color(color)).truncate());
            });
            help_button(ui, g, e);
        },
    );
}

/// The **?** of entry `e` of `generator`.
pub(super) fn help_button(ui: &mut egui::Ui, generator: &str, e: &'static Entry) {
    let r = ui
        .add(
            egui::Label::new(
                RichText::new("?")
                    .small()
                    .strong()
                    .color(super::theme::tokens().accent),
            )
            .sense(egui::Sense::click()),
        )
        .on_hover_ui(|ui| entry_ui(ui, e, None, None));
    if r.clicked() {
        let id = current_id(generator);
        ui.ctx()
            .data_mut(|d| d.insert_temp(id, (e.id.to_owned(), false)));
    }
}

/// A plain label with the **?** of input `id` after it (the Shaft
/// Generator's grids).
pub(super) fn label_help(ui: &mut egui::Ui, generator: &str, id: &str) {
    ui.horizontal(|ui| {
        ui.label(super::i18n::tr(id));
        if let Some(e) = explain::input(generator, id) {
            help_button(ui, generator, e);
        }
    });
}

/// The **?** of input `id` of `generator` alone (after a check box or a
/// heading).
pub(super) fn help_for(ui: &mut egui::Ui, generator: &str, id: &str) {
    if let Some(e) = explain::input(generator, id) {
        help_button(ui, generator, e);
    }
}

/// Hover on a result row: its entry, its formula and `numbers` (the formula
/// with the values in it); a click puts it in the pane.
pub(super) fn result_hover(
    r: egui::Response,
    generator: &str,
    id: &str,
    numbers: Option<&str>,
) -> egui::Response {
    let Some(e) = explain::result(generator, id) else {
        return r;
    };
    let f = explain::formula(generator, id);
    let clicked = r.clicked();
    let r = r.on_hover_ui(|ui| entry_ui(ui, e, f, numbers));
    if clicked {
        let key = current_id(generator);
        r.ctx
            .data_mut(|d| d.insert_temp(key, (e.id.to_owned(), true)));
    }
    r
}

/// The Explanations pane of `generator`: the entry chosen last (a **?** or a
/// result clicked), then the method's assumptions and every formula.
pub(super) fn pane(ui: &mut egui::Ui, generator: &str) {
    egui::CollapsingHeader::new(super::i18n::tr("Explanations"))
        .id_salt(("explain_pane", generator))
        .default_open(false)
        .show(ui, |ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            let cur: Option<(String, bool)> = ui.ctx().data(|d| d.get_temp(current_id(generator)));
            match cur.as_ref().and_then(|(id, res)| {
                if *res {
                    explain::result(generator, id)
                } else {
                    explain::input(generator, id)
                }
            }) {
                Some(e) => {
                    let f = explain::formula(generator, e.id);
                    entry_ui(ui, e, f, None);
                }
                None => {
                    ui.label(
                        RichText::new(super::i18n::tr(
                            "Click a ? beside a field, or a result, to read about it here",
                        ))
                        .weak()
                        .italics(),
                    );
                }
            }
            ui.separator();
            ui.label(RichText::new(super::i18n::tr("Method")).strong());
            ui.add(egui::Label::new(super::i18n::tr(explain::method(generator))).wrap());
            ui.label(RichText::new(super::i18n::tr("Formulas")).strong());
            for f in explain::formulas(generator) {
                ui.label(RichText::new(super::i18n::tr(f.id)).small().strong());
                formula_ui(ui, f);
            }
        });
}

/// The generator a planned panel explains (its command id).
pub(super) fn generator_of(d: &super::part_planned::PartDialog) -> Option<&'static str> {
    use super::asm_planned::AsmPlanned as A;
    use super::part_planned::Planned;
    match d.kind {
        Planned::Asm(A::SpurGear) => Some(explain::GEARS),
        Planned::Asm(A::Key) => Some(explain::KEY),
        Planned::Asm(A::Bearing) => Some(explain::BEARING),
        Planned::Asm(A::CompressionSpring) => Some(explain::SPRING),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::asm_planned::AsmPlanned as A;
    use super::super::part_planned::Row;
    use crate::design::explain::{self, BEARING, GEARS, KEY, SHAFT, SPRING};

    /// Every number, choice and check box of every calculating generator's
    /// panel has an entry, and so has every field of the Shaft Generator's
    /// Calculation tab (its `label_help` calls).
    #[test]
    fn every_field_of_every_generator_is_explained() {
        let mut missing = Vec::new();
        for (kind, g) in [
            (A::SpurGear, GEARS),
            (A::Key, KEY),
            (A::Bearing, BEARING),
            (A::CompressionSpring, SPRING),
        ] {
            for page in kind.pages() {
                for r in page.1 {
                    let label = match *r {
                        Row::Num(l, ..)
                        | Row::Combo(l, ..)
                        | Row::Radio(l, ..)
                        | Row::Check(l, _) => l,
                        _ => continue,
                    };
                    if explain::input(g, label).is_none() {
                        missing.push(format!("{g}: {label}"));
                    }
                }
            }
        }
        let src = include_str!("shaft.rs");
        let mut n = 0;
        let inputs = src
            .split("label_help(ui, SHAFT, \"")
            .skip(1)
            .chain(src.split("help_for(ui, SHAFT, \"").skip(1));
        for part in inputs {
            let label = part.split('"').next().unwrap_or("");
            n += 1;
            if explain::input(SHAFT, label).is_none() {
                missing.push(format!("{SHAFT}: {label}"));
            }
        }
        for label in super::super::shaft::MATERIAL_ROWS {
            if explain::input(SHAFT, label).is_none() {
                missing.push(format!("{SHAFT}: {label}"));
            }
        }
        let results = src
            .split("fn calc_results")
            .nth(1)
            .and_then(|r| {
                r.split(
                    "
fn ",
                )
                .next()
            })
            .unwrap_or("");
        for part in results.split("row(").skip(1) {
            let part = part.trim_start();
            let Some(rest) = part.strip_prefix('"') else {
                continue;
            };
            let label = rest.split('"').next().unwrap_or("");
            if explain::result(SHAFT, label).is_none() {
                missing.push(format!("{SHAFT} result: {label}"));
            }
        }
        assert!(
            n >= 15,
            "the Shaft Generator's fields go through label_help ({n})"
        );
        assert!(missing.is_empty(), "no explanation: {missing:#?}");
    }
}
