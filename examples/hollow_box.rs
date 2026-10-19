//! A hollow box: a 40 mm cube, a 32×32 pocket 36 deep cut from the top
//! (4 mm walls and bottom), its 8 inner edges (4 vertical, 4 at the bottom)
//! rounded r = 2. Optional argument 2: "chamfer" for a chamfer instead.
//!
//! ```sh
//! cargo run --example hollow_box -- /tmp/hollow.3dcadcraft
//! cargo run -- /tmp/hollow.3dcadcraft
//! ```

use std::path::PathBuf;

use cadcraft3d::document::{DocumentData, Project};
use cadcraft3d::kernel::Point3;
use cadcraft3d::part::blend::edge_ref;
use cadcraft3d::part::{
    BlendFeature, ExtentDirection, ExtrudeFeature, Operation, PartDocument, ProfileRef,
    SketchOptions, SketchPlacement,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path: PathBuf = std::env::args_os()
        .nth(1)
        .map_or("hollow_box.3dcadcraft".into(), Into::into);
    let chamfer = std::env::args().nth(2).as_deref() == Some("chamfer");
    let s = 40.0;
    let mut part = PartDocument::with_box("Box1", [s, s, s])?;
    let opts = SketchOptions {
        auto_project_origin: false,
        ..Default::default()
    };
    let i = part.create_sketch(SketchPlacement::on_face("Box1:Top"), &opts)?;
    let plane = part.sketches[i].sketch.plane.clone();
    let a = plane.project(&Point3::new(4.0, 4.0, s));
    let b = plane.project(&Point3::new(36.0, 36.0, s));
    let sk = &mut part.sketches[i].sketch;
    sk.add_rectangle_two_point(a, b)?;
    let r = sk.regions();
    // The inner square (the ring around it has a hole).
    let pocket = r.iter().find(|x| x.holes.is_empty()).ok_or("no region")?;
    let mut ex = ExtrudeFeature::new(vec![ProfileRef::of(pocket, pocket.interior_point())], 36.0);
    ex.direction = ExtentDirection::Flipped;
    ex.operation = Operation::Cut;
    part.add_extrude(i, ex)?;
    // The inner edges: inside the walls, below the rim.
    let inner: Vec<_> = part
        .body
        .edges()
        .filter_map(|(e, _)| edge_ref(&part.body, e))
        .filter(|r| {
            let [x, y, z] = r.hint;
            let inside = |v: f64| v > 3.0 && v < 37.0;
            z < s - 1e-6 && inside(x) && inside(y)
        })
        .collect();
    println!("{} inner edges", inner.len());
    let def = if chamfer {
        BlendFeature::chamfer(inner, 2.0)
    } else {
        BlendFeature::fillet(inner, 2.0)
    };
    part.add_blend(def)?;
    println!(
        "exact: {}, faces: {}",
        part.exact,
        part.body.faces().count()
    );
    let mut project = Project::new("HollowBox");
    project.add_document("Part1", DocumentData::Part(part));
    project.save(&path)?;
    println!("saved {}", path.display());
    Ok(())
}
