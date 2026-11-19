//! `3dcadcraft-mcp`: the MCP server of 3DCadCraft without the graphics stack (spec 0.7).
//!
//! Usage: `3dcadcraft-mcp [PROJECT]` — an MCP client starts it and talks JSON-RPC over
//! stdin / stdout; PROJECT (a `.3dcadcraft` file) is opened first when given.

use std::process::ExitCode;

use cadcraft3d_core::api::{Session, call, mcp};

fn main() -> ExitCode {
    let mut session = Session::default();
    if let Some(path) = std::env::args().nth(1).filter(|a| a != "--mcp")
        && let Err(e) = call(
            &mut session,
            "project_open",
            &serde_json::json!({ "path": path }),
        )
    {
        eprintln!("{e}");
        return ExitCode::FAILURE;
    }
    match mcp::serve(
        std::io::BufReader::new(std::io::stdin()),
        std::io::stdout(),
        session,
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("3dcadcraft-mcp: {e}");
            ExitCode::FAILURE
        }
    }
}
