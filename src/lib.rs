//! # 3DCadCraft
//!
//! An open-source, parametric 3D CAD system written in pure Rust.
//!
//! The crate is layered so that every layer below the GUI is usable headless,
//! which is what makes the application drivable by external AI agents:
//!
//! ```text
//!   ┌───────────────────────────── gui (egui + wgpu) ───┐  ┌─ api (JSON-RPC / MCP) ─┐
//!   └──────────────────────────────┬────────────────────┘  └───────────┬────────────┘
//!                                  ▼                                   ▼
//!                      command  (Command pattern, undo/redo, the only way to mutate)
//!                                  │
//!                 ┌────────────────┴────────────────┐
//!                 ▼                                 ▼
//!           sketch (2D + constraint solver)    kernel (B-Rep geometry & topology)
//! ```
//!
//! Phase 1 implements [`kernel`]; the other modules are placeholders that
//! document their intended responsibilities.

#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod api;
pub mod command;
pub mod document;
#[cfg(feature = "gui")]
pub mod gui;
pub mod kernel;
pub mod materials;
pub mod part;
pub mod presets;
pub mod sketch;
