//! Machine elements with their calculations (FEATURES §6.2, Inventor's Design
//! Accelerator; spec `docs/spec/6.2-design-calculators.md`).
//!
//! Everything here is pure: no GUI, no kernel calls except where a module
//! builds its part (`shaft::part`). Each formula cites its source — a free
//! document (spec §0): NASA Reference Publications 1123 (shafts) and 1152
//! (gears), and Kimball and Barr, *Elements of Machine Design* (1909, keys).
//! Units: N, mm, MPa (N/mm²), N·mm, radians, unless a function says otherwise.

pub mod gear;
pub mod key;
pub mod shaft;
pub mod shaft_calc;

/// Standard gravity, mm/s².
pub const G_MM_S2: f64 = 9_806.65;
