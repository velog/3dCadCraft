//! A plate with a sketch on its top (a rectangle and a ring), ready for Emboss.
//!
//! ```sh
//! cargo run --example emboss_demo -- /tmp/emboss.3dcadcraft
//! cargo run -- /tmp/emboss.3dcadcraft
//! ```

use std::path::PathBuf;

use cadcraft3d::document::{DocumentData, Project};
use cadcraft3d::part::work::WorkPlaneDef;
use cadcraft3d::part::{
    ExtrudeFeature, Operation, OriginPlane, PartDocument, PlaneRef, ProfileRef, SketchOptions,
    SketchPlacement, WorkDef, WorkFeature,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path: PathBuf = std::env::args_os()
        .nth(1)
        .map_or("emboss.3dcadcraft".into(), Into::into);
    let mut part = PartDocument::with_box("Box1", [100.0, 60.0, 20.0])?;
    let opts = SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    };
    let i = part.create_sketch(SketchPlacement::on_face("Box1:Top"), &opts)?;
    let plane = part.sketches[i].sketch.plane.clone();
    let p = |x: f64, y: f64| plane.project(&nalgebra::Point3::new(x, y, 20.0));
    let sk = &mut part.sketches[i].sketch;
    sk.add_rectangle_two_point(p(15.0, 20.0), p(45.0, 40.0))?;
    sk.add_circle(p(75.0, 30.0), 12.0)?;
    let mut roller = PartDocument::default();
    let i = roller.create_sketch(
        SketchPlacement {
            plane: PlaneRef::Origin(OriginPlane::XY),
            ..Default::default()
        },
        &opts,
    )?;
    let sk = &mut roller.sketches[i].sketch;
    sk.add_circle(nalgebra::Point2::new(0.0, 0.0), 20.0)?;
    let r: Vec<ProfileRef> = sk
        .regions()
        .iter()
        .map(|r| ProfileRef::of(r, r.interior_point()))
        .collect();
    let mut ex = ExtrudeFeature::new(r, 60.0);
    ex.operation = Operation::NewSolid;
    roller.add_extrude(i, ex)?;
    // A plane beside the cylinder, with a rectangle to wrap round it.
    let wp = roller.add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
        base: PlaneRef::Origin(OriginPlane::XZ),
        distance: -25.0,
    })))?;
    let i = roller.create_sketch(
        SketchPlacement {
            plane: PlaneRef::Work(wp),
            ..Default::default()
        },
        &opts,
    )?;
    let plane = roller.sketches[i].sketch.plane.clone();
    let p = |x: f64, z: f64| plane.project(&nalgebra::Point3::new(x, 25.0, z));
    roller.sketches[i]
        .sketch
        .add_rectangle_two_point(p(-15.0, 20.0), p(15.0, 40.0))?;
    let mut project = Project::new("Emboss");
    project.add_document("Plate", DocumentData::Part(part));
    project.add_document("Roller", DocumentData::Part(roller));
    project.save(&path)?;
    println!("saved {}", path.display());
    Ok(())
}
