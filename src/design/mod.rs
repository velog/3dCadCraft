//! Machine elements with their calculations (FEATURES §6.2, Inventor's Design
//! Accelerator; spec `docs/spec/6.2-design-calculators.md`).
//!
//! Everything here is pure: no GUI, no kernel calls except where a module
//! builds its part (`shaft::part`). Each formula cites its source — a free
//! document (spec §0): NASA Reference Publications 1123 (shafts), 1152
//! (gears) and 1105 (rolling bearings), and Kimball and Barr, *Elements of
//! Machine Design* (1909, keys and springs).
//! Units: N, mm, MPa (N/mm²), N·mm, radians, unless a function says otherwise.

pub mod bearing;
pub mod explain;
pub mod gear;
pub mod gear_model;
pub mod key;
pub mod shaft;
pub mod shaft_calc;
pub mod spring;
pub mod spring_model;

/// Standard gravity, mm/s².
pub const G_MM_S2: f64 = 9_806.65;

/// What a generator stored with the part it made (spec 6.2-design-calculators
/// §4), so the part can be edited again through the generator (Inventor: Edit
/// using Design Accelerator). The part itself is built from its features; this
/// is only the generator's input.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind")]
pub enum Generator {
    /// A shaft: its sections and the calculation's inputs.
    Shaft {
        /// The sections, bores and features.
        design: shaft::ShaftDesign,
        /// The calculation's inputs as last used.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        loads: Option<shaft_calc::ShaftLoads>,
    },
}

/// The input of a generator whose panel is the Design tab's planned panel
/// (Spur Gears, Parallel Key, Bearing, the springs; spec 6.2-design-calculators
/// §4): the panel's values as they were at OK, so Edit opens the generator on
/// them again. A field of its own (`PartDocument::element`), not a
/// [`Generator`] variant, so builds that do not know it still read the file.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Element {
    /// The command's id in the registry (`design.spur_gear`).
    pub command: String,
    /// Which element of the set this part is (gear 1 or 2).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub index: usize,
    /// The panel's values.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<ElementValue>,
    /// Sizes the part took from the assembly, not from the panel (a gear's
    /// `bore` from its picked cylinder, `hub_b` / `hub_t2` of a key's groove).
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub extra: std::collections::BTreeMap<String, f64>,
}

fn is_zero(v: &usize) -> bool {
    *v == 0
}

/// One value of a generator's panel: its tab, its row's label and the value.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ElementValue {
    /// The tab (0 = Design).
    pub tab: usize,
    /// The row's label.
    pub label: String,
    /// A number row's value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub num: Option<f64>,
    /// A drop-down or radio row's choice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
    /// A check box.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<bool>,
}
