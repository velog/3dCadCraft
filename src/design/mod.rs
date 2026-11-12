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
pub mod gear;
pub mod gear_model;
pub mod key;
pub mod shaft;
pub mod shaft_calc;
pub mod spring;

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
