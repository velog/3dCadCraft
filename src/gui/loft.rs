//! The Loft panel (`docs/spec/5.2-loft.md`): the Curves, Conditions and
//! Transition tabs, the selector flow (Sections → Rails / Center Line →
//! Map Point) shared with the other commands (`command_ui`), Area Loft's
//! placed sections and the end conditions. Picks in the view and the
//! preview are in `gui/sweep.rs`.

use eframe::egui::{self, RichText};

use super::CadCraftApp;
use super::command_ui::{MenuOption, RowEvents, selector_rows};
use super::icons::Icon;
use super::panels::{plabel, psection};
use super::sweep::{
    Button, LoftDraft, buttons, describe_path, describe_section, output_rows, solids_row,
};
use super::valuefield::ExprValue;
use crate::command::flow::{Flow, loft_selectors};
use crate::kernel::Real;
use crate::part::{
    ConditionKind, LoftCondition, LoftFeature, LoftGuide, LoftSection, MapPoint, PlacedSection,
};

/// The tabs of the Loft panel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum LoftTab {
    /// Sections, rails / centre line, Area Loft, output.
    #[default]
    Curves,
    /// End conditions.
    Conditions,
    /// Point mapping.
    Transition,
}

impl LoftDraft {
    /// A new loft: the Sections selector active.
    pub(super) fn new() -> Self {
        Self {
            def: LoftFeature::new(Vec::new()),
            edit: None,
            input: "sections",
            tab: LoftTab::Curves,
            map_set: 0,
        }
    }

    /// Edit Feature of `def` (feature `k`).
    pub(super) fn editing(def: LoftFeature, k: usize) -> Self {
        Self {
            def,
            edit: Some(k),
            ..Self::new()
        }
    }

    /// The point set being picked (Transition tab, manual mapping).
    pub(super) fn map_editing(&self) -> Option<usize> {
        (!self.def.automatic_mapping && self.map_set < self.def.map_points.len())
            .then_some(self.map_set)
    }

    /// The selectors with the active one.
    pub(super) fn flow(&self) -> Flow {
        Flow::new(
            loft_selectors(&self.def, self.map_editing()),
            Some(self.input),
        )
    }

    /// Makes the selector `key` active.
    pub(super) fn set_active(&mut self, key: &str) {
        if let Some(s) = loft_selectors(&self.def, self.map_editing())
            .into_iter()
            .find(|s| s.key == key)
        {
            self.input = s.key;
        }
    }

    /// A valid pick went in: a single-pick selector moves on.
    pub(super) fn picked(&mut self) {
        let mut f = self.flow();
        if f.picked()
            && let Some(k) = f.active_key()
        {
            self.input = k;
        }
    }

    /// Empties selector `key` (its `x`).
    fn clear(&mut self, key: &str) {
        match key {
            "sections" => self.def.sections.clear(),
            "rails" => self.def.rails.clear(),
            "center" => self.def.center_line = None,
            "map" => {
                if let Some(m) = self.def.map_points.get_mut(self.map_set) {
                    m.points.clear();
                }
            }
            _ => {}
        }
        self.set_active(key);
    }

    /// The right-click menu's options: the guide.
    pub(super) fn menu_options(&self) -> Vec<MenuOption> {
        [
            (LoftGuide::Rails, "Rails"),
            (LoftGuide::CenterLine, "Center Line"),
            (LoftGuide::AreaLoft, "Area Loft"),
        ]
        .into_iter()
        .map(|(g, label)| MenuOption {
            label: label.into(),
            checked: self.def.guide == g,
        })
        .collect()
    }

    /// Picks option `k` of [`Self::menu_options`].
    pub(super) fn menu_option(&mut self, k: usize) {
        if let Some(g) = [LoftGuide::Rails, LoftGuide::CenterLine, LoftGuide::AreaLoft].get(k) {
            self.set_guide(*g);
        }
    }

    fn set_guide(&mut self, g: LoftGuide) {
        if self.def.guide != g {
            self.def.guide = g;
            self.input = match g {
                LoftGuide::Rails => "rails",
                _ => "center",
            };
        }
    }

    /// The condition of section `k`, the list grown as needed.
    fn set_condition(&mut self, k: usize, c: LoftCondition) {
        if self.def.conditions.len() <= k {
            self.def.conditions.resize(k + 1, LoftCondition::default());
        }
        self.def.conditions[k] = c;
        while self
            .def
            .conditions
            .last()
            .is_some_and(|c| *c == LoftCondition::default())
        {
            self.def.conditions.pop();
        }
    }
}

impl CadCraftApp {
    /// The Loft panel.
    pub(super) fn loft_dialog(&mut self, ui: &mut egui::Ui) {
        let Some(mut d) = self.loft_draft.clone() else {
            return;
        };
        self.panel_header(ui, Icon::Loft, "Loft");
        ui.horizontal(|ui| {
            for (t, label) in [
                (LoftTab::Curves, "Curves"),
                (LoftTab::Conditions, "Conditions"),
                (LoftTab::Transition, "Transition"),
            ] {
                ui.selectable_value(&mut d.tab, t, RichText::new(label).strong());
            }
        });
        ui.separator();
        match d.tab {
            LoftTab::Curves => self.loft_curves_tab(ui, &mut d),
            LoftTab::Conditions => self.loft_conditions_tab(ui, &mut d),
            LoftTab::Transition => self.loft_transition_tab(ui, &mut d),
        }
        self.loft_draft = Some(d.clone());
        let preview = self.loft_preview();
        match buttons(ui, &preview, d.edit.is_none()) {
            Some(Button::Ok) => self.commit_loft(false),
            Some(Button::Apply) => self.commit_loft(true),
            Some(Button::Cancel) => {
                self.loft_draft = None;
                self.end_feature_edit();
            }
            None => {}
        }
    }

    fn loft_rows(&self, ui: &mut egui::Ui, d: &mut LoftDraft) {
        let part = self.part();
        let flow = d.flow();
        let texts = vec![
            ("sections", format!("{} section(s)", d.def.sections.len())),
            (
                "rails",
                if d.def.rails.is_empty() {
                    "none".into()
                } else {
                    format!("{} rail(s)", d.def.rails.len())
                },
            ),
            (
                "center",
                d.def
                    .center_line
                    .as_ref()
                    .map_or_else(String::new, |c| describe_path(c, part)),
            ),
            (
                "map",
                format!(
                    "{} / {} point(s)",
                    d.def
                        .map_points
                        .get(d.map_set)
                        .map_or(0, |m| m.points.len()),
                    d.def.sections.len()
                ),
            ),
        ];
        let mut ev = RowEvents::default();
        egui::Grid::new("loft_selectors")
            .num_columns(2)
            .show(ui, |ui| {
                ev = selector_rows(ui, &flow, &texts);
            });
        if let Some(k) = ev.activate {
            d.set_active(k);
        }
        if let Some(k) = ev.clear {
            d.clear(k);
        }
    }

    fn loft_curves_tab(&self, ui: &mut egui::Ui, d: &mut LoftDraft) {
        let part = self.part();
        let bodies = part.bodies_before(d.edit.unwrap_or(part.insert_index()));
        self.loft_rows(ui, d);
        let names: Vec<String> = d
            .def
            .sections
            .iter()
            .map(|s| describe_section(s, part))
            .collect();
        let mut remove = None;
        psection(ui, "loft_curves", "Curves", |ui| {
            plabel(ui, "Sections");
            ui.vertical(|ui| {
                if names.is_empty() {
                    ui.label(RichText::new("click in the view").italics());
                }
                for (k, n) in names.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(format!("{}. {n}", k + 1));
                        if ui.small_button("x").on_hover_text("Remove").clicked() {
                            remove = Some(k);
                        }
                    });
                }
            });
            ui.end_row();
            plabel(ui, "Guide");
            ui.vertical(|ui| {
                let mut g = d.def.guide;
                ui.radio_value(&mut g, LoftGuide::Rails, "Rails")
                    .on_hover_text("Curves that touch every section shape the loft between them");
                ui.radio_value(&mut g, LoftGuide::CenterLine, "Center Line")
                    .on_hover_text("The sections between stay square to one curve");
                ui.radio_value(&mut g, LoftGuide::AreaLoft, "Area Loft")
                    .on_hover_text("A centre line plus section areas along it");
                d.set_guide(g);
            });
            ui.end_row();
            if d.def.guide == LoftGuide::Rails && !d.def.rails.is_empty() {
                plabel(ui, "Rails");
                let mut drop = None;
                ui.vertical(|ui| {
                    for (j, r) in d.def.rails.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(format!("{}. {}", j + 1, describe_path(r, part)));
                            if ui.small_button("x").on_hover_text("Remove").clicked() {
                                drop = Some(j);
                            }
                        });
                    }
                });
                if let Some(j) = drop {
                    d.def.rails.remove(j);
                }
                ui.end_row();
            }
            plabel(ui, "Closed Loop");
            ui.checkbox(&mut d.def.closed_loop, "")
                .on_hover_text("The last section joins the first (at least three sections)");
            ui.end_row();
            plabel(ui, "Merge Tangent");
            ui.checkbox(&mut d.def.merge_tangent_faces, "Faces")
                .on_hover_text("Each segment gives one smooth face through every section");
            ui.end_row();
            if bodies.len() > 1 {
                solids_row(ui, &bodies, &mut d.def.solids);
            }
        });
        if let Some(k) = remove {
            d.def.sections.remove(k);
        }
        if d.def.guide == LoftGuide::AreaLoft {
            self.loft_area_section(ui, d);
        }
        psection(ui, "loft_output", "Output", |ui| {
            output_rows(ui, &mut d.def.output, &mut d.def.operation);
        });
    }

    /// Area Loft: the sections' areas and the placed sections.
    fn loft_area_section(&self, ui: &mut egui::Ui, d: &mut LoftDraft) {
        let part = self.part();
        let areas: Vec<Option<Real>> = (0..d.def.sections.len())
            .map(|k| d.def.section_area(&part.sketches, &part.body, k))
            .collect();
        psection(ui, "loft_area", "Section Dimensions", |ui| {
            for (k, a) in areas.iter().enumerate() {
                plabel(ui, &format!("Section {}", k + 1));
                ui.label(a.map_or_else(|| "-".into(), |a| format!("{a:.2} mm² (driven)")));
                ui.end_row();
            }
        });
        let mut drop = None;
        psection(ui, "loft_placed", "Placed Sections", |ui| {
            for (i, p) in d.def.placed.iter_mut().enumerate() {
                plabel(ui, &format!("{}", i + 1));
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label("Position");
                        if p.proportional {
                            ui.add(ExprValue::new(&mut p.position).range(0.0..=1.0));
                        } else {
                            ui.add(ExprValue::new(&mut p.position).suffix(" mm"));
                        }
                        ui.checkbox(&mut p.proportional, "Proportional")
                            .on_hover_text("A fraction of the centre line between the end sections (else mm from the first section)");
                    });
                    ui.horizontal(|ui| {
                        ui.label(if p.scale { "Scale" } else { "Area" });
                        ui.add(ExprValue::new(&mut p.size).range(1e-6..=1e9));
                        ui.checkbox(&mut p.scale, "Scale Factor")
                            .on_hover_text("Scale the section (else give its area in mm²)");
                    });
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut p.driving, "Driving")
                            .on_hover_text("A driving section sets the size there; a driven one only shows it");
                        if ui.small_button("x").on_hover_text("Remove").clicked() {
                            drop = Some(i);
                        }
                    });
                });
                ui.end_row();
            }
            plabel(ui, "");
            if ui
                .button("Add Placed Section")
                .on_hover_text("A section along the centre line whose area you set")
                .clicked()
            {
                let size = areas.first().copied().flatten().unwrap_or(100.0);
                d.def.placed.push(PlacedSection {
                    position: 0.5,
                    proportional: true,
                    size,
                    scale: false,
                    driving: true,
                });
            }
            ui.end_row();
        });
        if let Some(i) = drop {
            d.def.placed.remove(i);
        }
    }

    fn loft_conditions_tab(&self, ui: &mut egui::Ui, d: &mut LoftDraft) {
        let n = d.def.sections.len();
        if n < 2 {
            ui.label(RichText::new("Select the sections first").italics());
            return;
        }
        if d.def.closed_loop {
            ui.label("A closed loop has no end sections.");
            return;
        }
        let guided = !d.def.rails.is_empty() || d.def.guide != LoftGuide::Rails;
        for k in [0, n - 1] {
            let sec = d.def.sections[k].clone();
            let point = matches!(sec, LoftSection::Point { .. });
            let mut c = d.def.condition(k);
            psection(
                ui,
                &format!("loft_cond_{k}"),
                &format!("Section {}", k + 1),
                |ui| {
                    plabel(ui, "Condition");
                    ui.vertical(|ui| {
                    if point {
                        if c.kind == ConditionKind::Free {
                            c.kind = ConditionKind::SharpPoint;
                        }
                        ui.radio_value(&mut c.kind, ConditionKind::SharpPoint, "Sharp Point");
                        for t in ["Tangent", "Tangent to Plane"] {
                            ui.add_enabled(false, egui::RadioButton::new(false, t))
                                .on_disabled_hover_text(
                                    "Not supported yet: the kernel lofts to a point with straight sides only (docs/spec/5.2-loft.md)",
                                );
                        }
                    } else {
                        ui.radio_value(&mut c.kind, ConditionKind::Free, "Free");
                        let face = matches!(sec, LoftSection::FaceLoop(_));
                        let edges = matches!(sec, LoftSection::Edges(_));
                        ui.add_enabled_ui((face || edges) && !guided, |ui| {
                            ui.radio_value(&mut c.kind, ConditionKind::Tangent, "Tangent")
                        })
                        .response
                        .on_disabled_hover_text(if guided {
                            "Not with rails or a centre line yet"
                        } else {
                            "For a face loop or the edges of a surface: tangent to the faces next to them"
                        });
                        ui.add_enabled_ui(!face && !edges && !guided, |ui| {
                            ui.radio_value(&mut c.kind, ConditionKind::Direction, "Direction")
                        })
                        .response
                        .on_disabled_hover_text(if guided {
                            "Not with rails or a centre line yet"
                        } else {
                            "For sketch sections: an angle to the section plane"
                        });
                        ui.add_enabled(false, egui::RadioButton::new(false, "Smooth (G2)"))
                            .on_disabled_hover_text("Not supported yet (docs/spec/5.2-loft.md)");
                    }
                });
                    ui.end_row();
                    if c.kind == ConditionKind::Direction {
                        plabel(ui, "Angle");
                        let mut deg = c.angle.to_degrees();
                        if ui
                        .add(ExprValue::new(&mut deg).range(0.0..=180.0).suffix("°"))
                        .on_hover_text("90° leaves the section square; below leans inwards, 180° is flat outwards")
                        .changed()
                    {
                        c.angle = deg.to_radians();
                    }
                        ui.end_row();
                    }
                    if matches!(c.kind, ConditionKind::Direction | ConditionKind::Tangent) {
                        plabel(ui, "Weight");
                        ui.add(ExprValue::new(&mut c.weight).range(0.0..=50.0))
                        .on_hover_text("How far the condition reaches (× the distance to the next section)");
                        ui.end_row();
                    }
                },
            );
            if point && c.kind == ConditionKind::SharpPoint {
                c.kind = ConditionKind::Free;
            }
            d.set_condition(k, c);
        }
    }

    fn loft_transition_tab(&self, ui: &mut egui::Ui, d: &mut LoftDraft) {
        let n = d.def.sections.len();
        psection(ui, "loft_transition", "Transition", |ui| {
            plabel(ui, "Mapping");
            let mut auto = d.def.automatic_mapping;
            ui.checkbox(&mut auto, "Automatic Mapping")
                .on_hover_text("Off: pick one point on every section; the points line up");
            if auto != d.def.automatic_mapping {
                d.def.automatic_mapping = auto;
                if !auto && d.def.map_points.is_empty() {
                    d.def.map_points.push(MapPoint { points: Vec::new() });
                    d.map_set = 0;
                }
                if !auto {
                    d.input = "map";
                } else if d.input == "map" {
                    d.input = "sections";
                }
            }
            ui.end_row();
        });
        if d.def.automatic_mapping {
            ui.label(
                RichText::new("Vertices are paired by their direction from each section's centre.")
                    .italics(),
            );
            return;
        }
        let mut drop = None;
        psection(ui, "loft_point_sets", "Point Sets", |ui| {
            for (i, m) in d.def.map_points.iter().enumerate() {
                plabel(ui, &format!("Set {}", i + 1));
                ui.horizontal(|ui| {
                    let text = format!("{} / {n} points", m.points.len());
                    if ui
                        .selectable_label(d.map_set == i, text)
                        .on_hover_text("Pick into this set")
                        .clicked()
                    {
                        d.map_set = i;
                        d.input = "map";
                    }
                    if ui.small_button("x").on_hover_text("Remove").clicked() {
                        drop = Some(i);
                    }
                });
                ui.end_row();
            }
            plabel(ui, "");
            if ui.button("New Point Set").clicked() {
                d.def.map_points.push(MapPoint { points: Vec::new() });
                d.map_set = d.def.map_points.len() - 1;
                d.input = "map";
            }
            ui.end_row();
        });
        if let Some(i) = drop {
            d.def.map_points.remove(i);
            d.map_set = d.map_set.min(d.def.map_points.len().saturating_sub(1));
        }
        self.loft_rows(ui, d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::part::PathRef;

    #[test]
    fn loft_flow_guides_and_conditions() {
        let mut d = LoftDraft::new();
        let f = d.flow();
        assert_eq!(f.active_key(), Some("sections"));
        assert!(!f.ready());
        // The guide changes the second selector and makes it active.
        d.menu_option(1);
        assert_eq!(d.def.guide, LoftGuide::CenterLine);
        assert_eq!(d.input, "center");
        assert!(d.menu_options()[1].checked);
        let keys: Vec<_> = d.flow().selectors.iter().map(|s| s.key).collect();
        assert_eq!(keys, ["sections", "center"]);
        // Picking the centre line (a single pick) moves on (here: stays,
        // nothing follows) and Clear empties it.
        d.def.center_line = Some(PathRef::Edges(Vec::new()));
        d.picked();
        d.clear("center");
        assert!(d.def.center_line.is_none());
        assert_eq!(d.input, "center");
        // Back to rails; manual mapping adds the Map Point selector.
        d.menu_option(0);
        assert_eq!(d.input, "rails");
        d.def.automatic_mapping = false;
        d.def.map_points.push(MapPoint { points: Vec::new() });
        let keys: Vec<_> = d.flow().selectors.iter().map(|s| s.key).collect();
        assert_eq!(keys, ["sections", "rails", "map"]);
        d.set_active("map");
        assert_eq!(d.input, "map");
        d.set_active("nonsense");
        assert_eq!(d.input, "map");
        // Conditions: set at the end, trimmed back when Free again.
        let dir = LoftCondition {
            kind: ConditionKind::Direction,
            ..Default::default()
        };
        d.set_condition(2, dir);
        assert_eq!(d.def.conditions.len(), 3);
        assert_eq!(d.def.condition(2), dir);
        d.set_condition(2, LoftCondition::default());
        assert!(d.def.conditions.is_empty());
    }
}
