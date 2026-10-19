//! Exports the first part of a project to STL, STEP or GLB (by the output
//! file's extension), one or several at once.
//!
//! ```sh
//! cargo run --example export -- design.3dcadcraft design.stl design.step design.glb
//! ```

use std::path::PathBuf;

use cadcraft3d::document::{DocumentData, Project};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1).map(PathBuf::from);
    let input = args
        .next()
        .ok_or("usage: export <project.3dcadcraft> <out.stl|step|glb>...")?;
    let mut project = Project::load(&input)?;
    let id = project
        .documents()
        .iter()
        .find(|d| matches!(d.data, DocumentData::Part(_)))
        .map(|d| d.id)
        .ok_or("the project has no part")?;
    let Some(DocumentData::Part(part)) = project.document_mut(id).map(|d| &mut d.data) else {
        return Err("the project has no part".into());
    };
    part.regenerate()?;
    let outputs: Vec<PathBuf> = args.collect();
    if outputs.is_empty() {
        return Err("give at least one output file (.stl, .step or .glb)".into());
    }
    for out in outputs {
        let name = out
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Part")
            .to_owned();
        cadcraft3d::part::export::export(part, &name, &out)?;
        println!("exported {}", out.display());
    }
    Ok(())
}
