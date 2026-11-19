//! Automation API for external agents: an MCP server over JSON-RPC 2.0 (spec
//! `docs/spec/0.7-api-mcp.md`).
//!
//! - [`session`]: the state a client works on (a project, its file, the active document,
//!   Undo / Redo of whole calls).
//! - [`tools`]: the command layer: every tool, its input schema and the function running it
//!   on a [`Session`]. Tools call the same part / assembly operations as the GUI's commands
//!   (`PartDocument::add_extrude`, `add_hole`…), never geometry of their own. The GUI will
//!   expose the same table to an agent connected to the running app (spec §6).
//! - [`mcp`]: the transport: newline-delimited JSON-RPC over stdin / stdout, calls on a
//!   worker thread with progress, status and cancel.
//!
//! Run it with `3dcadcraft-mcp` (headless, `cargo build -p cadcraft3d-core --bin
//! 3dcadcraft-mcp`) or `3dcadcraft --mcp` (the desktop binary, no window).

pub mod args;
pub mod asm;
pub mod mcp;
pub mod part;
pub mod session;
pub mod tools;

pub use session::{ApiError, Session};
pub use tools::call;
