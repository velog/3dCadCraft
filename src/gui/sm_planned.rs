//! The interface of the sheet metal commands whose logic comes later (rule 15,
//! `docs/spec/5.6-sheet-metal.md` §3.5): Bend, Contour Roll, Lofted Flange,
//! Rip, Corner Round, Corner Chamfer and Punch Tool open their whole Inventor
//! 2027 property panel — selectors that take picks in the view, every option
//! with its default — so the panel can be checked before the geometry exists.
//! OK and Apply stay disabled: nothing changes the model (rule 5) until each
//! command's logic is written against its spec rows.
//!
//! The panel state lives in [`Planned`] (not saved; there is no feature
//! yet); `SmDraft::planned` carries it, so the command shares the sheet metal
//! command's selector rows, right-click menu, Esc and status prompt.

use eframe::egui;

use super::panels::{plabel, psection};
use super::valuefield::ExprValue;
use crate::command::flow::{PickKind, Selector};
use crate::kernel::Point3;
use crate::kernel::exact::sheet_metal::RuleValues as SheetRuleValues;

/// Selector dot colours (as the other sheet metal commands).
const C_INPUT: [u8; 3] = [40, 120, 230];
const C_SECOND: [u8; 3] = [200, 90, 40];

/// The command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PlannedKind {
    Bend,
    ContourRoll,
    LoftedFlange,
    Rip,
    CornerRound,
    CornerChamfer,
    Punch,
}

impl PlannedKind {
    pub(super) fn title(self) -> &'static str {
        match self {
            Self::Bend => "Bend",
            Self::ContourRoll => "Contour Roll",
            Self::LoftedFlange => "Lofted Flange",
            Self::Rip => "Rip",
            Self::CornerRound => "Corner Round",
            Self::CornerChamfer => "Corner Chamfer",
            Self::Punch => "Punch Tool",
        }
    }

    /// The prefix of the feature name the breadcrumb shows.
    pub(super) fn prefix(self) -> &'static str {
        match self {
            Self::Bend => "Bend",
            Self::ContourRoll => "ContourRoll",
            Self::LoftedFlange => "LoftedFlange",
            Self::Rip => "Rip",
            Self::CornerRound => "CornerRound",
            Self::CornerChamfer => "CornerChamfer",
            Self::Punch => "Punch",
        }
    }
}

/// Three-way side / direction buttons (Inventor's Flip Side, Both Sides).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Side {
    #[default]
    Default,
    Flipped,
    Both,
}

/// Corner Edge Set selector keys (a selector key is `'static`).
const SET_KEYS: [&str; 9] = [
    "set1", "set2", "set3", "set4", "set5", "set6", "set7", "set8", "set9",
];
/// Their row labels (a row's label is its widget id: they must differ).
const SET_LABELS: [&str; 9] = [
    "Corners 1",
    "Corners 2",
    "Corners 3",
    "Corners 4",
    "Corners 5",
    "Corners 6",
    "Corners 7",
    "Corners 8",
    "Corners 9",
];

/// One picked item: its id (to toggle) and its highlight lines.
pub(super) type Item = (String, Vec<Vec<Point3>>);

/// The panel state of a planned command.
#[derive(Clone, Debug)]
pub(super) struct Planned {
    pub kind: PlannedKind,
    /// Picks per selector key.
    pub picks: Vec<(&'static str, Vec<Item>)>,
    // Bend
    pub extend_aligned: bool,
    pub double_bend: u8,
    pub flip_fixed: bool,
    // Bend properties (Bend, Contour Roll, Lofted Flange)
    pub radius: Option<f64>,
    pub new_solid: bool,
    pub follow_defaults: bool,
    // Contour Roll
    pub roll_side: Side,
    pub roll_angle: f64,
    pub roll_dir: Side,
    pub unroll: u8,
    pub developed_length: f64,
    pub neutral_radius: f64,
    pub k_factor: f64,
    // Lofted Flange
    pub press_brake: bool,
    pub converge: bool,
    pub facet: u8,
    pub chord: f64,
    pub facet_angle: f64,
    pub facet_distance: f64,
    // Rip
    pub rip_type: u8,
    pub rip_gap: Option<f64>,
    pub gap_side: Side,
    // Corner Round
    pub feature_priority: bool,
    pub radii: Vec<f64>,
    // Corner Chamfer
    pub chamfer: u8,
    pub distance: f64,
    pub angle: f64,
    pub distance2: f64,
    pub chamfer_flip: bool,
    // Punch Tool
    pub punch_angle: f64,
    pub across_bend: bool,
}

impl Planned {
    /// A new panel with Inventor's defaults (lengths from the rule).
    pub(super) fn new(kind: PlannedKind, rule: &SheetRuleValues) -> Self {
        let t = rule.thickness;
        Self {
            kind,
            picks: Vec::new(),
            extend_aligned: false,
            double_bend: 0,
            flip_fixed: false,
            radius: None,
            new_solid: false,
            follow_defaults: true,
            roll_side: Side::Default,
            roll_angle: 90.0,
            roll_dir: Side::Default,
            unroll: 0,
            developed_length: 100.0,
            neutral_radius: 10.0,
            k_factor: rule.k_factor,
            press_brake: false,
            converge: false,
            facet: 0,
            chord: 0.1,
            facet_angle: 10.0,
            facet_distance: 10.0,
            rip_type: 0,
            rip_gap: None,
            gap_side: Side::Default,
            feature_priority: false,
            radii: vec![t * 2.0],
            chamfer: 0,
            distance: t * 2.0,
            angle: 45.0,
            distance2: t * 2.0,
            chamfer_flip: false,
            punch_angle: 0.0,
            across_bend: false,
        }
    }

    /// The items of selector `key`.
    pub(super) fn items(&self, key: &str) -> &[Item] {
        self.picks
            .iter()
            .find(|(k, _)| *k == key)
            .map_or(&[][..], |(_, v)| v.as_slice())
    }

    /// Adds `item` to selector `key` (a single selector replaces; picking
    /// a held item again removes it).
    pub(super) fn toggle(&mut self, key: &'static str, single: bool, item: Item) {
        let k = match self.picks.iter().position(|(k, _)| *k == key) {
            Some(k) => k,
            None => {
                self.picks.push((key, Vec::new()));
                self.picks.len() - 1
            }
        };
        let v = &mut self.picks[k].1;
        match v.iter().position(|(id, _)| *id == item.0) {
            Some(i) => {
                v.remove(i);
            }
            None if single => *v = vec![item],
            None => v.push(item),
        }
    }

    /// Clears selector `key` (its `x`).
    pub(super) fn clear(&mut self, key: &str) {
        self.picks.retain(|(k, _)| *k != key);
    }

    /// Every held item's lines (drawn as chosen).
    pub(super) fn chosen(&self) -> Vec<Vec<Point3>> {
        self.picks
            .iter()
            .flat_map(|(_, v)| v.iter().flat_map(|(_, l)| l.iter().cloned()))
            .collect()
    }

    /// Inventor's Input Geometry rows.
    pub(super) fn selectors(&self) -> Vec<Selector> {
        let sel = |key: &'static str,
                   label: &'static str,
                   prompt: &'static str,
                   accepts: &'static [PickKind],
                   single: bool,
                   second: bool| Selector {
            key,
            label,
            prompt,
            accepts,
            single,
            required: true,
            count: self.items(key).len(),
            color: if second { C_SECOND } else { C_INPUT },
        };
        match self.kind {
            PlannedKind::Bend => vec![
                sel(
                    "edge1",
                    "Edge 1",
                    "click an edge of the first face",
                    &[PickKind::Edge],
                    true,
                    false,
                ),
                sel(
                    "edge2",
                    "Edge 2",
                    "click the matching edge of the second face",
                    &[PickKind::Edge],
                    true,
                    true,
                ),
            ],
            PlannedKind::ContourRoll => vec![
                sel(
                    "profile",
                    "Profile",
                    "click the open profile (a sketch chain)",
                    &[PickKind::SketchCurve],
                    true,
                    false,
                ),
                sel(
                    "axis",
                    "Axis",
                    "click the axis, a line of the same sketch",
                    &[PickKind::SketchCurve],
                    true,
                    true,
                ),
            ],
            PlannedKind::LoftedFlange => vec![
                sel(
                    "profile1",
                    "Profile 1",
                    "click the first profile (sketch geometry)",
                    &[PickKind::SketchCurve],
                    true,
                    false,
                ),
                sel(
                    "profile2",
                    "Profile 2",
                    "click the second profile (sketch geometry)",
                    &[PickKind::SketchCurve],
                    true,
                    true,
                ),
            ],
            PlannedKind::Rip => {
                let mut v = vec![sel(
                    "face",
                    "Rip Face",
                    "click the sheet face to rip",
                    &[PickKind::Face],
                    true,
                    false,
                )];
                match self.rip_type {
                    0 => v.push(sel(
                        "point",
                        "Rip Point",
                        "click the rip point (vertex, edge midpoint or sketch point)",
                        &[PickKind::Point, PickKind::SketchPoint],
                        true,
                        true,
                    )),
                    1 => {
                        v.push(sel(
                            "start",
                            "Start Point",
                            "click the point where the rip starts",
                            &[PickKind::Point, PickKind::SketchPoint],
                            true,
                            true,
                        ));
                        v.push(sel(
                            "end",
                            "End Point",
                            "click the point where the rip ends",
                            &[PickKind::Point, PickKind::SketchPoint],
                            true,
                            true,
                        ));
                    }
                    _ => {}
                }
                v
            }
            PlannedKind::CornerRound => {
                let mut v = Vec::new();
                if self.feature_priority {
                    v.push(sel(
                        "feature",
                        "Feature",
                        "click a sheet metal feature: all its corners",
                        &[PickKind::Face],
                        false,
                        false,
                    ));
                } else {
                    for (k, key) in SET_KEYS.iter().enumerate().take(self.radii.len()) {
                        v.push(sel(
                            key,
                            SET_LABELS[k],
                            "click the corner edges to round (a second click removes one)",
                            &[PickKind::Edge],
                            false,
                            k % 2 == 1,
                        ));
                    }
                }
                v
            }
            PlannedKind::CornerChamfer => {
                let single = self.chamfer == 2;
                let mut v = vec![sel(
                    "corners",
                    "Corners",
                    if single {
                        "click the corner edge to chamfer"
                    } else {
                        "click the corner edges to chamfer (a second click removes one)"
                    },
                    &[PickKind::Edge],
                    single,
                    false,
                )];
                if self.chamfer == 1 {
                    v.push(sel(
                        "edge",
                        "Edge",
                        "click the sheet edge the angle is measured from",
                        &[PickKind::Edge],
                        true,
                        true,
                    ));
                }
                v
            }
            PlannedKind::Punch => vec![sel(
                "centers",
                "Centers",
                "click the sketch points to punch at (a second click removes one)",
                &[PickKind::SketchPoint],
                false,
                false,
            )],
        }
    }

    /// Why OK is not there yet.
    pub(super) const NOT_YET: &'static str = "Interface only: the geometry of this command \
         comes after the panel is checked (docs/spec/5.6-sheet-metal.md)";

    /// The command's own sections (below Input Geometry).
    pub(super) fn panel(&mut self, ui: &mut egui::Ui, rule: &SheetRuleValues) {
        match self.kind {
            PlannedKind::Bend => self.bend(ui, rule),
            PlannedKind::ContourRoll => self.contour_roll(ui, rule),
            PlannedKind::LoftedFlange => self.lofted_flange(ui, rule),
            PlannedKind::Rip => self.rip(ui, rule),
            PlannedKind::CornerRound => self.corner_round(ui),
            PlannedKind::CornerChamfer => self.corner_chamfer(ui),
            PlannedKind::Punch => self.punch(ui),
        }
    }

    fn bend(&mut self, ui: &mut egui::Ui, rule: &SheetRuleValues) {
        psection(ui, "smp_bend_ext", "Bend Extension", |ui| {
            plabel(ui, "Extend Bend");
            ui.vertical(|ui| {
                ui.radio_value(
                    &mut self.extend_aligned,
                    false,
                    "Perpendicular to Side Face",
                )
                .on_hover_text("The new material runs square to the bend (default)");
                ui.radio_value(&mut self.extend_aligned, true, "Aligned to Side Face")
                    .on_hover_text("The new material follows the side faces the bend joins");
            });
            ui.end_row();
        });
        psection(ui, "smp_bend_double", "Double Bend", |ui| {
            plabel(ui, "Type");
            ui.vertical(|ui| {
                for (k, (name, tip)) in [
                    (
                        "Fix Edges",
                        "Two equal bends between the faces; a face between them when they are far apart",
                    ),
                    ("45 Degree", "Two 45° bends; the second face is trimmed or extended"),
                    (
                        "Full Radius",
                        "One half-round bend of half the faces' distance; the faces trimmed or extended",
                    ),
                    ("90 Degree", "Two 90° bends; the second face is trimmed or extended"),
                ]
                .into_iter()
                .enumerate()
                {
                    ui.radio_value(&mut self.double_bend, k as u8, name)
                        .on_hover_text(tip);
                }
            });
            ui.end_row();
            plabel(ui, "Fixed Edge");
            ui.checkbox(&mut self.flip_fixed, "Flip Fixed Edge")
                .on_hover_text(
                    "Keep the second edge and trim or extend the first (off: the first stays)",
                );
            ui.end_row();
            plabel(ui, "");
            ui.weak("parallel faces only");
            ui.end_row();
        });
        self.bend_properties(ui, rule, true);
    }

    /// Bend Radius (rule or own) and the rule's other values.
    fn bend_properties(&mut self, ui: &mut egui::Ui, rule: &SheetRuleValues, relief: bool) {
        psection(ui, "smp_bend_props", "Bend Properties", |ui| {
            plabel(ui, "Bend Radius");
            ui.horizontal(|ui| {
                let mut own = self.radius.is_some();
                if ui
                    .checkbox(&mut own, "")
                    .on_hover_text("Off: the rule's bend radius")
                    .changed()
                {
                    self.radius = own.then_some(rule.bend_radius);
                }
                match &mut self.radius {
                    Some(r) => {
                        ui.add(ExprValue::new(r).range(0.0..=1e6).suffix(" mm"));
                    }
                    None => {
                        ui.weak(format!("{:.3} mm (rule)", rule.bend_radius));
                    }
                }
            });
            ui.end_row();
            plabel(ui, "Unfold Rule");
            ui.weak(format!("Default (K-factor {:.2})", rule.k_factor));
            ui.end_row();
            if relief {
                plabel(ui, "Relief Shape");
                ui.weak("Default (rule)");
                ui.end_row();
            }
        });
    }

    fn contour_roll(&mut self, ui: &mut egui::Ui, rule: &SheetRuleValues) {
        psection(ui, "smp_roll_shape", "Shape", |ui| {
            plabel(ui, "Side");
            side_row(
                ui,
                "smp_roll_side",
                &mut self.roll_side,
                "The thickness on the other side of the profile",
                "The thickness centred on the profile",
            );
            ui.end_row();
            plabel(ui, "Rolled Angle");
            ui.add(
                ExprValue::new(&mut self.roll_angle)
                    .range(0.0..=360.0)
                    .suffix("°"),
            )
            .on_hover_text("360° only for a profile of one straight line");
            ui.end_row();
            plabel(ui, "Direction");
            side_row(
                ui,
                "smp_roll_dir",
                &mut self.roll_dir,
                "Roll the other way round the axis",
                "Half the angle each way from the profile",
            );
            ui.end_row();
        });
        psection(ui, "smp_roll_unroll", "Unrolling", |ui| {
            plabel(ui, "Method");
            let methods = [
                (
                    "Centroid Cylinder",
                    "The neutral cylinder through the profile's centroid (default)",
                ),
                (
                    "Custom Cylinder",
                    "A sketch line gives the neutral cylinder",
                ),
                ("Developed Length", "The flat length given as a value"),
                (
                    "Neutral Radius",
                    "The neutral radius given as a value (profiles of several segments)",
                ),
                ("K-Factor", "A K-factor (a profile of one straight line)"),
            ];
            egui::ComboBox::from_id_salt("smp_unroll")
                .selected_text(methods[self.unroll as usize].0)
                .show_ui(ui, |ui| {
                    for (k, (name, tip)) in methods.iter().enumerate() {
                        ui.selectable_value(&mut self.unroll, k as u8, *name)
                            .on_hover_text(*tip);
                    }
                });
            ui.end_row();
            match self.unroll {
                2 => {
                    plabel(ui, "Developed Length");
                    ui.add(
                        ExprValue::new(&mut self.developed_length)
                            .range(0.0..=1e7)
                            .suffix(" mm"),
                    );
                    ui.end_row();
                }
                3 => {
                    plabel(ui, "Neutral Radius");
                    ui.add(
                        ExprValue::new(&mut self.neutral_radius)
                            .range(0.0..=1e7)
                            .suffix(" mm"),
                    );
                    ui.end_row();
                }
                4 => {
                    plabel(ui, "K-Factor");
                    ui.add(ExprValue::new(&mut self.k_factor).range(0.0..=1.0));
                    ui.end_row();
                }
                1 => {
                    plabel(ui, "Neutral Line");
                    ui.weak("a sketch line (selector comes with the logic)");
                    ui.end_row();
                }
                _ => {}
            }
            plabel(ui, "Unfold Rule");
            ui.weak("Default (rule)");
            ui.end_row();
            plabel(ui, "Result");
            ui.weak("neutral radius, length: -")
                .on_hover_text("The neutral radius and the unrolled length, once computed");
            ui.end_row();
        });
        self.bend_properties(ui, rule, false);
        self.output(ui);
    }

    fn lofted_flange(&mut self, ui: &mut egui::Ui, rule: &SheetRuleValues) {
        psection(ui, "smp_loft_rule", "Sheet Metal Rule", |ui| {
            plabel(ui, "Rule");
            ui.checkbox(&mut self.follow_defaults, "Follow Defaults")
                .on_hover_text("The part's thickness and rule (off: a rule of its own)");
            ui.end_row();
            plabel(ui, "Thickness");
            ui.weak(format!("{:.3} mm", rule.thickness));
            ui.end_row();
        });
        psection(ui, "smp_loft_output", "Shape", |ui| {
            plabel(ui, "Output");
            ui.vertical(|ui| {
                ui.radio_value(&mut self.press_brake, false, "Die Form")
                    .on_hover_text("A smooth formed transition");
                ui.radio_value(&mut self.press_brake, true, "Press Brake")
                    .on_hover_text("Flat facets joined by bends, for a press brake");
            });
            ui.end_row();
            if self.press_brake {
                plabel(ui, "Bends");
                ui.checkbox(&mut self.converge, "Converge")
                    .on_hover_text("The facets' bends meet near one point in the flat");
                ui.end_row();
                plabel(ui, "Facet Control");
                let ways = [
                    (
                        "Chord Tolerance",
                        "Largest distance between the arc and a facet's chord",
                    ),
                    (
                        "Facet Angle",
                        "Largest angle between a facet's chord and the arc",
                    ),
                    ("Facet Distance", "Largest width of a facet"),
                ];
                egui::ComboBox::from_id_salt("smp_facet")
                    .selected_text(ways[self.facet as usize].0)
                    .show_ui(ui, |ui| {
                        for (k, (name, tip)) in ways.iter().enumerate() {
                            ui.selectable_value(&mut self.facet, k as u8, *name)
                                .on_hover_text(*tip);
                        }
                    });
                ui.end_row();
                plabel(ui, "Value");
                match self.facet {
                    0 => ui.add(
                        ExprValue::new(&mut self.chord)
                            .range(0.0..=1e6)
                            .suffix(" mm"),
                    ),
                    1 => ui.add(
                        ExprValue::new(&mut self.facet_angle)
                            .range(0.0..=180.0)
                            .suffix("°"),
                    ),
                    _ => ui.add(
                        ExprValue::new(&mut self.facet_distance)
                            .range(0.0..=1e6)
                            .suffix(" mm"),
                    ),
                };
                ui.end_row();
            }
            plabel(ui, "Note");
            ui.weak("fixed after OK")
                .on_hover_text("Die Form / Press Brake cannot be changed once created");
            ui.end_row();
        });
        self.bend_properties(ui, rule, false);
        self.output(ui);
    }

    fn rip(&mut self, ui: &mut egui::Ui, rule: &SheetRuleValues) {
        psection(ui, "smp_rip", "Rip", |ui| {
            plabel(ui, "Rip Type");
            let types = [
                ("Single Point", "A gap through one point, across the face"),
                (
                    "Point to Point",
                    "A gap along the line between two points of the face",
                ),
                ("Face Extents", "The whole face is removed (usually a bend)"),
            ];
            let before = self.rip_type;
            egui::ComboBox::from_id_salt("smp_rip_type")
                .selected_text(types[self.rip_type as usize].0)
                .show_ui(ui, |ui| {
                    for (k, (name, tip)) in types.iter().enumerate() {
                        ui.selectable_value(&mut self.rip_type, k as u8, *name)
                            .on_hover_text(*tip);
                    }
                });
            if self.rip_type != before {
                // Other points for another type.
                self.picks.retain(|(k, _)| *k == "face");
            }
            ui.end_row();
            if self.rip_type != 2 {
                plabel(ui, "Gap");
                ui.horizontal(|ui| {
                    let mut own = self.rip_gap.is_some();
                    if ui
                        .checkbox(&mut own, "")
                        .on_hover_text("Off: the rule's gap (GapSize)")
                        .changed()
                    {
                        self.rip_gap = own.then_some(rule.gap);
                    }
                    match &mut self.rip_gap {
                        Some(g) => {
                            ui.add(ExprValue::new(g).range(0.0..=1e6).suffix(" mm"));
                        }
                        None => {
                            ui.weak(format!("{:.3} mm (GapSize)", rule.gap));
                        }
                    }
                });
                ui.end_row();
                plabel(ui, "Gap Side");
                side_row(
                    ui,
                    "smp_rip_side",
                    &mut self.gap_side,
                    "The gap on the other side of the point",
                    "The gap centred on the point",
                );
                ui.end_row();
            }
        });
    }

    fn corner_round(&mut self, ui: &mut egui::Ui) {
        psection(ui, "smp_round", "Corner Round", |ui| {
            plabel(ui, "Select");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.feature_priority, false, "Corner")
                    .on_hover_text("Pick corner edges one by one (or by a window)");
                ui.selectable_value(&mut self.feature_priority, true, "Feature")
                    .on_hover_text(
                        "Pick a feature: all its corners, except where it meets other sheet faces",
                    );
            });
            ui.end_row();
            let n = self.radii.len();
            let mut remove = None;
            for (k, r) in self.radii.iter_mut().enumerate() {
                plabel(ui, &format!("Radius {}", k + 1));
                ui.horizontal(|ui| {
                    ui.add(ExprValue::new(r).range(0.0..=1e6).suffix(" mm"))
                        .on_hover_text(format!("The radius of corner edge set {}", k + 1));
                    if n > 1
                        && ui
                            .small_button("x")
                            .on_hover_text("Remove this set")
                            .clicked()
                    {
                        remove = Some(k);
                    }
                });
                ui.end_row();
            }
            if let Some(k) = remove {
                self.radii.remove(k);
                // The sets after it move up.
                let mut picks = std::mem::take(&mut self.picks);
                picks.retain(|(key, _)| *key != SET_KEYS[k]);
                for (key, _) in picks.iter_mut() {
                    if let Some(i) = SET_KEYS.iter().position(|s| s == key)
                        && i > k
                    {
                        *key = SET_KEYS[i - 1];
                    }
                }
                self.picks = picks;
            }
            plabel(ui, "");
            if ui
                .add_enabled(
                    !self.feature_priority && n < SET_KEYS.len(),
                    egui::Button::new("Add Corner Edge Set"),
                )
                .on_hover_text("Another set of corners with a radius of its own")
                .clicked()
            {
                let last = self.radii.last().copied().unwrap_or(1.0);
                self.radii.push(last);
            }
            ui.end_row();
        });
    }

    fn corner_chamfer(&mut self, ui: &mut egui::Ui) {
        psection(ui, "smp_chamfer", "Corner Chamfer", |ui| {
            plabel(ui, "Method");
            let before = self.chamfer;
            ui.vertical(|ui| {
                for (k, (name, tip)) in [
                    (
                        "One Distance",
                        "The same offset along both edges of each corner",
                    ),
                    (
                        "Distance and Angle",
                        "An offset along a chosen sheet edge and an angle from it",
                    ),
                    (
                        "Two Distances",
                        "One corner, a distance along each of its edges",
                    ),
                ]
                .into_iter()
                .enumerate()
                {
                    ui.radio_value(&mut self.chamfer, k as u8, name)
                        .on_hover_text(tip);
                }
            });
            if self.chamfer != before {
                self.picks.clear();
            }
            ui.end_row();
            match self.chamfer {
                0 => {
                    plabel(ui, "Distance");
                    ui.add(
                        ExprValue::new(&mut self.distance)
                            .range(0.0..=1e6)
                            .suffix(" mm"),
                    );
                    ui.end_row();
                }
                1 => {
                    plabel(ui, "Distance");
                    ui.add(
                        ExprValue::new(&mut self.distance)
                            .range(0.0..=1e6)
                            .suffix(" mm"),
                    );
                    ui.end_row();
                    plabel(ui, "Angle");
                    ui.add(
                        ExprValue::new(&mut self.angle)
                            .range(0.0..=180.0)
                            .suffix("°"),
                    );
                    ui.end_row();
                }
                _ => {
                    plabel(ui, "Distance 1");
                    ui.add(
                        ExprValue::new(&mut self.distance)
                            .range(0.0..=1e6)
                            .suffix(" mm"),
                    );
                    ui.end_row();
                    plabel(ui, "Distance 2");
                    ui.add(
                        ExprValue::new(&mut self.distance2)
                            .range(0.0..=1e6)
                            .suffix(" mm"),
                    );
                    ui.end_row();
                    plabel(ui, "Direction");
                    ui.checkbox(&mut self.chamfer_flip, "Flip")
                        .on_hover_text("Swap the two distances between the edges");
                    ui.end_row();
                }
            }
        });
    }

    fn punch(&mut self, ui: &mut egui::Ui) {
        psection(ui, "smp_punch_preview", "Punch", |ui| {
            plabel(ui, "Location");
            ui.weak("Sheet Metal Punch library")
                .on_hover_text("The folder of punch iFeatures (the library comes with 6.3)");
            ui.end_row();
            plabel(ui, "Punch");
            egui::ComboBox::from_id_salt("smp_punch")
                .selected_text("(no punches yet)")
                .show_ui(ui, |ui| {
                    ui.weak("The punch library (iFeatures, 6.3) is not there yet");
                });
            ui.end_row();
        });
        psection(ui, "smp_punch_geometry", "Geometry", |ui| {
            plabel(ui, "Angle");
            ui.add(
                ExprValue::new(&mut self.punch_angle)
                    .range(-360.0..=360.0)
                    .suffix("°"),
            )
            .on_hover_text("The punch turned from its own position");
            ui.end_row();
            plabel(ui, "Bends");
            ui.checkbox(&mut self.across_bend, "Across Bend")
                .on_hover_text("The punch runs on over a bend (off: it stops at the bend)");
            ui.end_row();
        });
        psection(ui, "smp_punch_size", "Size", |ui| {
            plabel(ui, "Parameters");
            ui.weak("none (no punch)")
                .on_hover_text("The chosen punch's parameters, name and value");
            ui.end_row();
        });
    }

    /// New Solid (a part that has a body already).
    fn output(&mut self, ui: &mut egui::Ui) {
        psection(ui, "smp_output", "Output", |ui| {
            plabel(ui, "Solids");
            ui.checkbox(&mut self.new_solid, "New Solid")
                .on_hover_text("A body of its own (off: joined to the sheet)");
            ui.end_row();
        });
    }
}

/// Default / Flipped / Both Sides buttons.
fn side_row(ui: &mut egui::Ui, id: &str, side: &mut Side, flip_tip: &str, both_tip: &str) {
    ui.push_id(id, |ui| {
        ui.horizontal(|ui| {
            ui.selectable_value(side, Side::Default, "Default")
                .on_hover_text("As previewed");
            // Short names: the value column is narrow.
            ui.selectable_value(side, Side::Flipped, "Flip")
                .on_hover_text(flip_tip);
            ui.selectable_value(side, Side::Both, "Both")
                .on_hover_text(format!("Both Sides: {both_tip}"));
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule() -> SheetRuleValues {
        crate::part::sheet_metal::SheetMetalDefaults::default().values()
    }

    #[test]
    fn selectors_follow_the_options() {
        let r = rule();
        let mut p = Planned::new(PlannedKind::Rip, &r);
        let keys = |p: &Planned| p.selectors().iter().map(|s| s.key).collect::<Vec<_>>();
        assert_eq!(keys(&p), ["face", "point"]);
        p.rip_type = 1;
        assert_eq!(keys(&p), ["face", "start", "end"]);
        p.rip_type = 2;
        assert_eq!(keys(&p), ["face"]);
        let mut c = Planned::new(PlannedKind::CornerChamfer, &r);
        assert_eq!(keys(&c), ["corners"]);
        c.chamfer = 1;
        assert_eq!(keys(&c), ["corners", "edge"]);
        let mut round = Planned::new(PlannedKind::CornerRound, &r);
        round.radii.push(3.0);
        assert_eq!(keys(&round), ["set1", "set2"]);
        round.feature_priority = true;
        assert_eq!(keys(&round), ["feature"]);
    }

    #[test]
    fn picks_toggle_and_single_replaces() {
        let mut p = Planned::new(PlannedKind::Bend, &rule());
        let item = |s: &str| (s.to_owned(), Vec::new());
        p.toggle("edge1", true, item("a"));
        p.toggle("edge1", true, item("b"));
        assert_eq!(p.items("edge1").len(), 1);
        assert_eq!(p.items("edge1")[0].0, "b");
        p.toggle("edge1", true, item("b"));
        assert!(p.items("edge1").is_empty());
        let mut q = Planned::new(PlannedKind::Punch, &rule());
        q.toggle("centers", false, item("p1"));
        q.toggle("centers", false, item("p2"));
        assert_eq!(q.selectors()[0].count, 2);
    }
}
