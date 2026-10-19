//! A disc (r 20, 5 thick) with a sketch on its top: a circle r 15 and a
//! radial line patterned 8 times (circular sketch pattern). Open it and cut
//! every other sector (Extrude, Cut, Through All) — a radial grille.
//!
//! ```sh
//! cargo run --example grille -- /tmp/grille.3dcadcraft
//! cargo run -- /tmp/grille.3dcadcraft
//! ```

use std::path::PathBuf;

use cadcraft3d::document::{DocumentData, Project};
use cadcraft3d::part::{
    ExtrudeFeature, OriginPlane, PartDocument, ProfileRef, SketchOptions, SketchPlacement,
};
use cadcraft3d::sketch::{Entity, Point2};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path: PathBuf = std::env::args_os()
        .nth(1)
        .map_or("grille.3dcadcraft".into(), Into::into);
    let mut part = PartDocument::default();
    let i = part.create_sketch(
        SketchPlacement::on_origin_plane(OriginPlane::XY),
        &SketchOptions::default(),
    )?;
    part.sketches[i]
        .sketch
        .add_circle(Point2::new(0.0, 0.0), 20.0)?;
    let r = part.sketches[i].sketch.regions();
    let disc = r
        .iter()
        .max_by(|a, b| a.area().total_cmp(&b.area()))
        .ok_or("no region")?;
    part.add_extrude(
        i,
        ExtrudeFeature::new(vec![ProfileRef::of(disc, disc.interior_point())], 5.0),
    )?;
    let j = part.create_sketch(
        SketchPlacement::on_face("Extrusion1:End"),
        &SketchOptions::default(),
    )?;
    let sk = &mut part.sketches[j].sketch;
    let c = sk.add_circle(Point2::new(0.0, 0.0), 15.0)?;
    let Some(Entity::Circle { center, .. }) = sk.entity(c).cloned() else {
        return Err("circle".into());
    };
    let b = sk.add_point(Point2::new(18.0, 0.0));
    let l = sk.add_line_between(center, b)?;
    sk.circular_pattern(&[l], center, 8, std::f64::consts::TAU, true)?;
    sk.solve()?;
    println!("regions in the top sketch: {}", sk.regions().len());
    let mut project = Project::new("Grille");
    project.add_document("Part1", DocumentData::Part(part));
    project.save(&path)?;
    println!("saved {}", path.display());
    Ok(())
}
