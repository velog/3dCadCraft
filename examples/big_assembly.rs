//! Builds a large assembly for speed tests: one plate (80 × 60 × 6 with a
//! hole) in every 20 occurrences, the others bolts (a 10 × 10 head with a
//! Ø6 × 20 shank), on a grid — like `tests/perf_drawing.rs`.
//!
//! `cargo run --example big_assembly -- out.3dcadcraft 5000`

use cadcraft3d::document::{AssemblyDocument, DocRef, DocumentData, Occurrence, Project};
use cadcraft3d::kernel::{Point3, Real, Transform3};
use cadcraft3d::part::{
    Extent, ExtentDirection, ExtrudeFeature, Operation, PartDocument, ProfileRef, SketchOptions,
    SketchPlacement,
};

/// A box with a circle cut through it or a boss on top.
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
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .unwrap_or_else(|| "big_assembly.3dcadcraft".into());
    let n: usize = args.next().map_or(Ok(5000), |s| s.parse())?;
    let mut project = Project::new("BigAssembly");
    let plate = project.add_document(
        "Plate",
        DocumentData::Part(box_with_circle([80.0, 60.0, 6.0], 8.0, true, 1.0)?),
    );
    let bolt = project.add_document(
        "Bolt",
        DocumentData::Part(box_with_circle([10.0, 10.0, 2.0], 3.0, false, 20.0)?),
    );
    let side = (n as f64).sqrt().ceil() as usize;
    let mut occurrences = Vec::with_capacity(n);
    for k in 0..n {
        let (i, j) = ((k % side) as f64, (k / side) as f64);
        let is_plate = k % 20 == 0;
        let (doc, base) = if is_plate {
            (plate, "Plate")
        } else {
            (bolt, "Bolt")
        };
        let mut o = Occurrence::new(
            format!("{base}:{}", k + 1),
            DocRef::local(doc),
            Transform3::translation(i * 90.0, j * 70.0, if is_plate { 0.0 } else { 6.0 }),
        );
        o.id = k as u32 + 1;
        o.grounded = true;
        occurrences.push(o);
    }
    project.add_document(
        "Assembly",
        DocumentData::Assembly(AssemblyDocument {
            occurrences,
            constraints: Vec::new(),
        }),
    );
    project.save(std::path::Path::new(&path))?;
    println!("saved {path}: {n} components");
    Ok(())
}
