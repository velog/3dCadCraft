//! Converts an imported STEP sheet the way Sheet Metal › Convert does with no edge picked and
//! prints what was recognised: thickness, bends, flat size, volumes.
//!
//! ```sh
//! cargo run --release --example sheet_recognize -- sheet.step <face name> <x> <y> <z>
//! ```

use cadcraft3d::part::import::ImportOp;
use cadcraft3d::part::sheet_metal::SheetConvertFeature;
use cadcraft3d::part::shell::FaceRef;
use cadcraft3d::part::{BodyOp, BodyOpFeature, PartDocument};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() < 5 {
        return Err("usage: sheet_recognize <file.step> <face> <x> <y> <z>".into());
    }
    let text = String::from_utf8_lossy(&std::fs::read(&a[0])?).into_owned();
    let mut p = PartDocument::default();
    p.make_sheet_metal(None);
    p.add_body_op(BodyOpFeature::new(BodyOp::Import(ImportOp::new(
        &a[0],
        text.as_str(),
    ))))?;
    let before = p
        .exact_bodies
        .0
        .iter()
        .filter_map(|s| s.volume_at(1e-3))
        .sum::<f64>();
    println!(
        "imported: {} faces, volume {:?}",
        p.body.faces().count(),
        before
    );
    let out = std::env::var("PNG_DIR").ok();
    let shot = |p: &PartDocument, name: &str| {
        if let Some(d) = &out {
            for (k, v) in [(1.0, -1.0, 1.0), (-1.0, 1.0, -1.0)].iter().enumerate() {
                let view = cadcraft3d::kernel::Vector3::new(v.0, v.1, v.2);
                if let Some(png) = cadcraft3d::part::thumbnail::part_png(p, view) {
                    let _ = std::fs::write(format!("{d}/{name}{k}.png"), png);
                }
            }
        }
    };
    shot(&p, "before");
    let hint = [a[2].parse()?, a[3].parse()?, a[4].parse()?];
    let body = p.exact_bodies.0[0].name.clone().ok_or("no body")?;
    let t = std::time::Instant::now();
    p.add_sheet_convert(SheetConvertFeature {
        id: String::new(),
        name: String::new(),
        body,
        face: FaceRef {
            face: a[1].clone(),
            hint,
        },
        bends: Vec::new(),
        recognize: true,
    })?;
    println!(
        "converted in {:.0} ms: {} faces, volume {:?}, health {:?}",
        t.elapsed().as_secs_f64() * 1e3,
        p.body.faces().count(),
        p.exact_bodies
            .0
            .iter()
            .filter_map(|s| s.volume_at(1e-3))
            .sum::<f64>(),
        p.feature_health
    );
    shot(&p, "after");
    for fp in p.flat_patterns() {
        println!(
            "flat {:.3} x {:.3}, {} bends",
            fp.size.0,
            fp.size.1,
            fp.bends.len()
        );
        for b in &fp.bends {
            println!("  {b:?}");
        }
    }
    Ok(())
}
