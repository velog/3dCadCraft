//! Public automation API for external agents (JSON-RPC 2.0 and MCP).
//!
//! **Status: placeholder (Phase 1, later step).**
//!
//! Planned design:
//! - A transport-agnostic dispatcher that maps JSON-RPC methods / MCP tools onto
//!   [`crate::command`] commands, so an agent can do everything the GUI can,
//!   with undo/redo, without a window.
//! - Transports: stdio (MCP's default) and a local socket / HTTP endpoint, runnable
//!   from a headless binary built with `--no-default-features`.
