//! A small assembly with overlapping components, to try Analyze
//! Interference: a plate (80 × 60 × 6 with a Ø16 hole), a pin too thick
//! for the hole (Ø20) standing in it, a block sunk 2 mm into the plate and
//! one resting on it (touching: no interference).
//!
//! `cargo run --example interference_demo -- out.3dcadcraft`

use cadcraft3d::document::{AssemblyDocument, DocRef, DocumentData, Occurrence, Project};
use cadcraft3d::kernel::{Point3, Real, Transform3};
use cadcraft3d::part::{
    Extent, ExtentDirection, ExtrudeFeature, Operation, PartDocument, ProfileRef, SketchOptions,
    SketchPlacement,
};

/// A box with a circle cut through it, or a box with a boss on top.
fn box_with_circle(
    size: [Real; 3],
    r: Real,
    cut: bool,
    h: Real,
) -> Result<PartDocument, Box<dyn std::error::Error>> {
    let mut part = PartDocument::with_box("Box1", size)?;
    let opts = SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    };
    let j = part.create_sketch(SketchPlacement::on_face("Box1:Top"), &opts)?;
    let c =
        part.sketches[j]
            .sketch
            .plane
            .project(&Point3::new(size[0] * 0.5, size[1] * 0.5, size[2]));
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
    part.add_extrude(j, ex)?;
    Ok(part)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "interference_demo.3dcadcraft".into());
    let mut project = Project::new("InterferenceDemo");
    let plate = project.add_document(
        "Plate",
        DocumentData::Part(box_with_circle([80.0, 60.0, 6.0], 8.0, true, 1.0)?),
    );
    let pin = project.add_document(
        "Pin",
        DocumentData::Part(box_with_circle([24.0, 24.0, 4.0], 10.0, false, 20.0)?),
    );
    let block = project.add_document(
        "Block",
        DocumentData::Part(PartDocument::with_box("Box1", [20.0, 15.0, 10.0])?),
    );
    let place = [
        (plate, "Plate", Transform3::identity()),
        // The pin's head on the plate, its Ø20 shank down through the Ø16 hole.
        (
            pin,
            "Pin",
            Transform3::translation(28.0, 42.0, 20.0)
                * Transform3::rotation(cadcraft3d::kernel::Vector3::x() * std::f64::consts::PI),
        ),
        (block, "Block", Transform3::translation(5.0, 5.0, 4.0)),
        (block, "Block", Transform3::translation(55.0, 40.0, 6.0)),
    ];
    let mut occurrences = Vec::new();
    for (k, (doc, base, t)) in place.into_iter().enumerate() {
        let n = occurrences
            .iter()
            .filter(|o: &&Occurrence| o.name.starts_with(base))
            .count();
        let mut o = Occurrence::new(format!("{base}:{}", n + 1), DocRef::local(doc), t);
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
        }),
    );
    project.save(std::path::Path::new(&path))?;
    println!("saved {path}");
    Ok(())
}
