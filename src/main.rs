//! 3DCadCraft desktop application entry point.
//!
//! Usage: `3dcadcraft [PROJECT]`, where PROJECT is a `.3dcadcraft` file or an
//! unpacked project folder; `3dcadcraft --mcp` serves MCP on stdin / stdout instead.

fn main() -> eframe::Result {
    // `3dcadcraft --mcp`: the MCP server for AI agents on stdin / stdout, no window (spec 0.7).
    if std::env::args().nth(1).as_deref() == Some("--mcp") {
        if let Err(e) = cadcraft3d::api::mcp::serve_stdio() {
            eprintln!("3dcadcraft --mcp: {e}");
        }
        return Ok(());
    }
    // First of all: the session log, the action journal and the panic hook
    // (crash reports, emergency autosave), before eframe starts.
    cadcraft3d::diagnostics::start(cadcraft3d::diagnostics::root());
    let result = cadcraft3d::gui::run(std::env::args_os().nth(1).map(Into::into));
    if let Err(e) = &result {
        log::error!("the window could not run: {e}");
    }
    cadcraft3d::diagnostics::finish();
    result
}
