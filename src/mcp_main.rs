//! `3dcadcraft-mcp`: the MCP server of 3DCadCraft without the graphics stack (spec 0.7).
//!
//! Usage: `3dcadcraft-mcp [--read-only] [--attach] [PROJECT]` — an MCP client starts it and
//! talks JSON-RPC over stdin / stdout; PROJECT (a `.3dcadcraft` file) is opened first when given;
//! `--attach` forwards to the running app when it allows AI connections.

use std::process::ExitCode;

use cadcraft3d_core::api::mcp;

fn main() -> ExitCode {
    let run = mcp::Options::parse(std::env::args().skip(1)).and_then(|o| mcp::run(&o));
    match run {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("3dcadcraft-mcp: {e}");
            ExitCode::FAILURE
        }
    }
}
