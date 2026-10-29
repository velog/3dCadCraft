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
    let mut each = false;
    let mut args = std::env::args_os().skip(1);
    // `--each`: every solid of the file read on its own (finding the one that hangs).
    while let Some(a) = args.next() {
        if a == "--each" {
            each = true;
        } else if a == "--save" {
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
        let text = String::from_utf8_lossy(&std::fs::read(&f)?).into_owned();
        if each {
            // Every solid on its own, 20 s each (a solid that does not finish is reported and
            // left running).
            for (id, t) in cadcraft3d::assembly::import::solid_texts(&text) {
                let faces = t.matches("ADVANCED_FACE").count();
                let (tx, rx) = std::sync::mpsc::channel();
                let start = Instant::now();
                std::thread::spawn(move || {
                    let _ = tx.send(
                        cadcraft3d::kernel::exact::step_bodies(&t, "S").map(|r| r.bodies.len()),
                    );
                });
                match rx.recv_timeout(std::time::Duration::from_secs(20)) {
                    Ok(r) => println!(
                        "solid #{id}: {faces} faces, {:.2} s, {:?}",
                        start.elapsed().as_secs_f64(),
                        r
                    ),
                    Err(_) => println!("solid #{id}: {faces} faces, NOT DONE after 20 s"),
                }
            }
            continue;
        }
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
        if let Ok(r) = cadcraft3d::kernel::exact::step_bodies(&text, "Import1") {
            for b in &r.bodies {
                println!(
                    "  body {:?} item {} label {} closed {}",
                    b.solid.name, b.item, b.label, b.closed
                );
            }
            println!("tree:");
            for n in &r.tree {
                print_tree(n, 1);
            }
            if cadcraft3d::assembly::import::is_assembly(&r) {
                let t = Instant::now();
                let name = f
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let mut p = Project::new("Imported");
                match cadcraft3d::assembly::import::step_project(&r, &text, &name, &mut p) {
                    Ok(out) => {
                        println!(
                            "assembly: {:.2} s, {} parts, {} assemblies, {} occurrences",
                            t.elapsed().as_secs_f64(),
                            out.parts,
                            out.assemblies,
                            out.occurrences
                        );
                        for w in &out.warnings {
                            println!("  warning: {w}");
                        }
                        for d in p.documents() {
                            if let DocumentData::Part(part) = &d.data {
                                let v: f64 = part
                                    .exact_bodies
                                    .0
                                    .iter()
                                    .filter_map(|s| s.volume_at(1e-3))
                                    .sum();
                                let mut again = part.clone();
                                again.build_cache = Default::default();
                                let t = Instant::now();
                                let _ = again.regenerate();
                                println!(
                                    "  part {:<14} volume {v:.1}  health {:?}  rebuild {:.2} s",
                                    d.name,
                                    part.feature_health,
                                    t.elapsed().as_secs_f64()
                                );
                            }
                        }
                        // Placement check: each top-level occurrence's centre of mass against its body's in
                        // the file.
                        if let (Some(top), [root]) = (out.top, r.tree.as_slice())
                            && let Some(DocumentData::Assembly(a)) =
                                p.document(top).map(|d| &d.data)
                        {
                            for (o, c) in a.occurrences.iter().zip(&root.children) {
                                let Some(&bi) = c.bodies.first() else {
                                    continue;
                                };
                                let want = r.bodies[bi].solid.measure_full().map(|m| m.centroid);
                                let got = match p.document(o.component.document).map(|d| &d.data) {
                                    Some(DocumentData::Part(part)) => part
                                        .exact_bodies
                                        .0
                                        .first()
                                        .and_then(|s| s.measure_full().ok())
                                        .map(|m| o.transform * m.centroid),
                                    _ => None,
                                };
                                if let (Ok(w), Some(g)) = (want, got) {
                                    println!("  {:<14} off by {:.4} mm", o.name, (w - g).norm());
                                }
                            }
                        }
                        if let Some(out_path) = &save {
                            let asm = out_path.with_extension("asm.3dcadcraft");
                            p.save(&asm)?;
                            println!("saved {}", asm.display());
                        }
                    }
                    Err(e) => println!("assembly: error {e}"),
                }
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

fn print_tree(n: &cadcraft3d::kernel::exact::StepProduct, depth: usize) {
    let t = n.transform;
    println!(
        "{}{}  bodies {:?}  at ({:.1}, {:.1}, {:.1})",
        "  ".repeat(depth),
        n.name,
        n.bodies,
        t[(0, 3)],
        t[(1, 3)],
        t[(2, 3)]
    );
    for c in &n.children {
        print_tree(c, depth + 1);
    }
}
