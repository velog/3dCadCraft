//! Reads STEP files the way File → Open / Import does and prints what came in: bodies (solid or
//! surface, faces, volume), the reader's warnings and the time each step took. Optionally saves
//! the imported part.
//!
//! ```sh
//! cargo run --release --example step_info -- model.step [more.stp ...] [--save out.3dcadcraft]
//! ```

use std::path::PathBuf;
use std::time::Instant;

use cadcraft3d::document::{DocumentData, Project};
use cadcraft3d::part::import::ImportOp;
use cadcraft3d::part::{BodyOp, BodyOpFeature, PartDocument};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut files = Vec::new();
    let mut save: Option<PathBuf> = None;
    let mut args = std::env::args_os().skip(1);
    while let Some(a) = args.next() {
        if a == "--save" {
            save = args.next().map(PathBuf::from);
        } else {
            files.push(PathBuf::from(a));
        }
    }
    if files.is_empty() {
        return Err("usage: step_info <file.step>... [--save out.3dcadcraft]".into());
    }
    for f in files {
        println!("== {}", f.display());
        let text = std::fs::read_to_string(&f)?;
        let t = Instant::now();
        let op = ImportOp::new(f.display().to_string(), text.as_str());
        match op.read("Import1") {
            Ok((bodies, warnings)) => {
                println!(
                    "read: {:.2} s, {} bodies",
                    t.elapsed().as_secs_f64(),
                    bodies.len()
                );
                for b in &bodies {
                    let m = b.measure();
                    println!(
                        "  {:<12} {:<8} faces {:>5}  volume {}",
                        b.name.as_deref().unwrap_or("?"),
                        if b.is_open() { "surface" } else { "solid" },
                        b.names().len(),
                        m.map(|(v, _, _)| format!("{v:.1}")).unwrap_or_else(|e| e)
                    );
                }
                for w in warnings.iter().take(20) {
                    println!("  warning: {w}");
                }
                if warnings.len() > 20 {
                    println!("  … {} more warnings", warnings.len() - 20);
                }
            }
            Err(e) => {
                println!("error: {e}");
                continue;
            }
        }
        let t = Instant::now();
        let mut part = PartDocument::default();
        match part.add_body_op(BodyOpFeature::new(BodyOp::Import(op))) {
            Ok(_) => println!(
                "part: {:.2} s, health {:?}",
                t.elapsed().as_secs_f64(),
                part.feature_health
            ),
            Err(e) => println!("part: error {e}"),
        }
        if let Some(out) = &save {
            let mut p = Project::new("Imported");
            p.add_document("Part1", DocumentData::Part(part));
            p.save(out)?;
            println!("saved {}", out.display());
        }
    }
    Ok(())
}
