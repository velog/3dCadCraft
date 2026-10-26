//! Writes the flat pattern of every sheet metal body of a project's first part
//! as DXF (outline on `OUTER`, bend lines on `BEND_UP` / `BEND_DOWN`), and
//! lists the rebuild notes of its sheet features.
//!
//! ```sh
//! cargo run --example flat_dxf -- design.3dcadcraft out_dir
//! ```

use std::path::PathBuf;

use cadcraft3d::document::{DocumentData, Project};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1).map(PathBuf::from);
    let input = args
        .next()
        .ok_or("usage: flat_dxf <project.3dcadcraft> [out_dir]")?;
    let out = args.next().unwrap_or_else(|| PathBuf::from("."));
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
    for (k, v) in &part.feature_health {
        println!("health {k}: {v:?}");
    }
    std::fs::create_dir_all(&out)?;
    let bodies: Vec<String> = part.sheet_models.keys().cloned().collect();
    for body in bodies {
        let text = part.flat_pattern_dxf(&body)?;
        let path = out.join(format!("{body}.dxf"));
        std::fs::write(&path, text)?;
        println!("wrote {}", path.display());
    }
    Ok(())
}
