//! A small assembly to try Pattern Component: a plate (100 × 60 × 8) with a
//! row of four Ø8 holes made by a rectangular feature pattern (20 mm apart),
//! a bolt standing in the first hole and a block on the plate.
//!
//! `cargo run --example pattern_demo -- out.3dcadcraft`

use cadcraft3d::document::{AssemblyDocument, DocRef, DocumentData, Occurrence, Project};
use cadcraft3d::kernel::{Point3, Real, Transform3, Vector3};
use cadcraft3d::part::{
    Extent, ExtentDirection, ExtrudeFeature, LineRef, Operation, PartDocument, PatternFeature,
    PatternKind, ProfileRef, PullRef, SketchOptions, SketchPlacement,
};

/// A box with a circle on its top, cut through (a hole) or extruded up `h`
/// (a boss).
fn box_with_circle(
    size: [Real; 3],
    at: [Real; 2],
    r: Real,
    cut: bool,
    h: Real,
) -> Result<(PartDocument, String), Box<dyn std::error::Error>> {
    let mut part = PartDocument::with_box("Box1", size)?;
    let opts = SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    };
    let j = part.create_sketch(SketchPlacement::on_face("Box1:Top"), &opts)?;
    let c = part.sketches[j]
        .sketch
        .plane
        .project(&Point3::new(at[0], at[1], size[2]));
    part.sketches[j].sketch.add_circle(c, r)?;
    let rg = part.sketches[j].sketch.regions();
    let first = rg.first().ok_or("no profile")?;
    let mut ex = ExtrudeFeature::new(vec![ProfileRef::of(first, first.interior_point())], h);
    if cut {
        ex.operation = Operation::Cut;
        ex.extent = Extent::ThroughAll;
        ex.direction = ExtentDirection::Flipped;
    } else {
        ex.operation = Operation::Join;
    }
    let id = part.add_extrude(j, ex)?;
    Ok((part, id))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "pattern_demo.3dcadcraft".into());
    let mut project = Project::new("PatternDemo");
    let (mut plate, hole) = box_with_circle([100.0, 60.0, 8.0], [20.0, 20.0], 4.0, true, 1.0)?;
    let mut p = PatternFeature::rectangular();
    p.features = vec![hole];
    if let PatternKind::Rectangular { first, .. } = &mut p.kind {
        first.dir = Some(PullRef::Line(LineRef::Origin(0)));
        first.count = 4;
        first.spacing = 20.0;
    }
    plate.add_pattern(p)?;
    let plate = project.add_document("Plate", DocumentData::Part(plate));
    let (bolt, _) = box_with_circle([14.0, 14.0, 5.0], [7.0, 7.0], 4.0, false, 20.0)?;
    let bolt = project.add_document("Bolt", DocumentData::Part(bolt));
    let block = project.add_document(
        "Block",
        DocumentData::Part(PartDocument::with_box("Box1", [20.0, 12.0, 10.0])?),
    );
    let place = [
        (plate, "Plate", Transform3::identity()),
        // The head on the plate, the shank down through the first hole.
        (
            bolt,
            "Bolt",
            Transform3::translation(13.0, 27.0, 13.0)
                * Transform3::rotation(Vector3::x() * std::f64::consts::PI),
        ),
        (block, "Block", Transform3::translation(10.0, 40.0, 8.0)),
    ];
    let mut occurrences = Vec::new();
    for (k, (doc, base, t)) in place.into_iter().enumerate() {
        let mut o = Occurrence::new(format!("{base}:1"), DocRef::local(doc), t);
        o.id = k as u32 + 1;
        o.grounded = k == 0;
        occurrences.push(o);
    }
    project.add_document(
        "Assembly",
        DocumentData::Assembly(AssemblyDocument {
            occurrences,
            constraints: Vec::new(),
            patterns: Vec::new(),
            appearances: Vec::new(),
            frames: Vec::new(),
        }),
    );
    project.save(std::path::Path::new(&path))?;
    println!("saved {path}");
    Ok(())
}
