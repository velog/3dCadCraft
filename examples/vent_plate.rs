//! A 120×60×5 plate with a sketch on its top face holding 40 rectangles
//! (10 × 4) and the face's projected edges — ready for one Extrude Cut:
//! Extrude takes all 40 rectangles as its starting profiles.
//!
//! ```sh
//! cargo run --example vent_plate -- /tmp/vent.3dcadcraft
//! cargo run -- /tmp/vent.3dcadcraft
//! ```

use std::path::PathBuf;

use cadcraft3d::document::{DocumentData, Project};
use cadcraft3d::kernel::Point3;
use cadcraft3d::part::{PartDocument, SketchOptions, SketchPlacement};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path: PathBuf = std::env::args_os()
        .nth(1)
        .map_or("vent_plate.3dcadcraft".into(), Into::into);
    let mut part = PartDocument::with_box("Box1", [120.0, 60.0, 5.0])?;
    let opts = SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: true,
        ..Default::default()
    };
    let i = part.create_sketch(SketchPlacement::on_face("Box1:Top"), &opts)?;
    let plane = part.sketches[i].sketch.plane.clone();
    for a in 0..10 {
        for b in 0..4 {
            let (x, y) = (6.0 + 11.0 * a as f64, 6.0 + 13.0 * b as f64);
            let p = plane.project(&Point3::new(x, y, 5.0));
            let q = plane.project(&Point3::new(x + 7.0, y + 9.0, 5.0));
            part.sketches[i].sketch.add_rectangle_two_point(p, q)?;
        }
    }
    println!("{} regions", part.sketches[i].sketch.regions().len());
    let mut project = Project::new("VentPlate");
    project.add_document("Part1", DocumentData::Part(part));
    project.save(&path)?;
    println!("saved {}", path.display());
    Ok(())
}
