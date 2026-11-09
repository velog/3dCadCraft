//! A freeform surface (spec 5.5): a control cage of 8 × 5 faces whose points
//! are pulled into a shallow, wavy shell.
//!
//! `cargo run --example freeform_demo -- out.3dcadcraft`

use cadcraft3d::document::{DocumentData, Project};
use cadcraft3d::part::freeform::FreeformOp;
use cadcraft3d::part::{BodyOp, BodyOpFeature, OriginPlane, PartDocument, PlaneRef};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: freeform_demo out.3dcadcraft")?;
    let mut op = FreeformOp::plane(PlaneRef::Origin(OriginPlane::XY), 240.0, 150.0);
    (op.length_faces, op.width_faces) = (8, 5);
    let (nu, nv) = (9, 6); // one more point than faces, each way
    for i in 0..nu {
        for j in 0..nv {
            let (u, v) = (i as f64 / (nu - 1) as f64, j as f64 / (nv - 1) as f64);
            let dome = 70.0 * (u * std::f64::consts::PI).sin() * (v * std::f64::consts::PI).sin();
            let wave = 14.0 * (u * 1.5 * std::f64::consts::TAU).sin();
            op.set_offset(
                i,
                j,
                [0.0, 0.0, dome + wave * (v * std::f64::consts::PI).sin()],
            );
        }
    }
    let mut part = PartDocument::default();
    part.add_body_op(BodyOpFeature::new(BodyOp::Freeform(op)))?;
    if !part.feature_health.is_empty() {
        return Err(format!("{:?}", part.feature_health).into());
    }
    let mut project = Project::new("Freeform");
    project.add_document("Shell", DocumentData::Part(part));
    project.save(std::path::Path::new(&path))?;
    println!("saved {path}");
    Ok(())
}
