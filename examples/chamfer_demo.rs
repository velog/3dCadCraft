//! A 40 × 30 × 20 block with three chamfered upright edges (equal 2 mm,
//! 2 mm × 30°, 3 / 5 mm) and a drawing with its top view at 2 : 1, for
//! trying Annotate › Chamfer Note.
//!
//! ```sh
//! cargo run --example chamfer_demo -- /tmp/chamfers.3dcadcraft
//! cargo run -- /tmp/chamfers.3dcadcraft      # open it, then the drawing
//! ```

use std::path::PathBuf;

use cadcraft3d::document::{DocRef, DocumentData, Project};
use cadcraft3d::drawing::{DrawingDocument, ViewOrientation, ViewStyle};
use cadcraft3d::part::PartDocument;
use cadcraft3d::part::blend::{BlendFeature, BlendKind, ChamferMethod, EdgeRef};

fn upright(a: &str, b: &str, x: f64, y: f64) -> EdgeRef {
    EdgeRef {
        faces: [a.into(), b.into()],
        hint: [x, y, 10.0],
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path: PathBuf = std::env::args_os()
        .nth(1)
        .map_or("chamfers.3dcadcraft".into(), Into::into);
    let mut part = PartDocument::with_box("Box1", [40.0, 30.0, 20.0])?;
    part.add_blend(BlendFeature::chamfer(
        vec![upright("Box1:Left", "Box1:Front", 0.0, 0.0)],
        2.0,
    ))?;
    let mut c = BlendFeature::chamfer(vec![upright("Box1:Front", "Box1:Right", 40.0, 0.0)], 2.0);
    if let BlendKind::Chamfer { method, angle, .. } = &mut c.kind {
        *method = ChamferMethod::DistanceAngle;
        *angle = 30f64.to_radians();
    }
    part.add_blend(c)?;
    let mut c = BlendFeature::chamfer(vec![upright("Box1:Right", "Box1:Back", 40.0, 30.0)], 3.0);
    if let BlendKind::Chamfer {
        method, distance2, ..
    } = &mut c.kind
    {
        *method = ChamferMethod::TwoDistances;
        *distance2 = 5.0;
    }
    part.add_blend(c)?;

    let mut project = Project::new("Chamfers");
    let part_id = project.add_document("Block", DocumentData::Part(part));
    let mut d = DrawingDocument::default();
    d.add_base_view(
        DocRef::local(part_id),
        ViewOrientation::Top,
        [150.0, 150.0],
        2.0,
        ViewStyle::HiddenRemoved,
    )
    .ok_or("base view")?;
    project.add_document("Drawing", DocumentData::Drawing(d));
    project.save(&path)?;
    println!("{}", path.display());
    Ok(())
}
