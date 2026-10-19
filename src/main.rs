//! 3DCadCraft desktop application entry point.
//!
//! Usage: `3dcadcraft [PROJECT]`, where PROJECT is a `.3dcadcraft` file or an
//! unpacked project folder.

fn main() -> eframe::Result {
    cadcraft3d::gui::run(std::env::args_os().nth(1).map(Into::into))
}
