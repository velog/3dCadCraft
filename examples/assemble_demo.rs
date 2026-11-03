//! An assembly to try Assemble, relationship glyphs, degrees of freedom and BOM numbers
//! (spec 3.1 §3.2a, §3.2b, spec 3.6): a grounded plate, free blocks and pins standing apart,
//! a Mate that leaves a block free in the plate's plane, and one relationship whose face
//! no longer exists (sick).
//!
//! `cargo run --example assemble_demo -- out.3dcadcraft`

use cadcraft3d::assembly::relation::{Constraint, ConstraintKind, GeomRef, OriginItem};
use cadcraft3d::document::{AssemblyDocument, DocRef, DocumentData, Project};
use cadcraft3d::kernel::{Real, Transform3, Vector3};
use cadcraft3d::part::{
    ExtrudeFeature, OriginPlane, PartDocument, ProfileRef, SketchOptions, SketchPlacement,
};
use cadcraft3d::sketch::Point2;

fn pin() -> Result<PartDocument, Box<dyn std::error::Error>> {
    let mut part = PartDocument::default();
    let opts = SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    };
    let i = part.create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts)?;
    part.sketches[i]
        .sketch
        .add_circle(Point2::new(0.0, 0.0), 6.0)?;
    let regions = part.sketches[i].sketch.regions();
    part.add_extrude(
        i,
        ExtrudeFeature::new(
            vec![ProfileRef::of(&regions[0], regions[0].interior_point())],
            25.0,
        ),
    )?;
    Ok(part)
}

fn at(x: Real, y: Real, z: Real) -> Transform3 {
    Transform3::new(Vector3::new(x, y, z), Vector3::zeros())
}

fn relation(kind: ConstraintKind, a: GeomRef, b: GeomRef) -> Constraint {
    Constraint {
        id: 0,
        name: String::new(),
        kind,
        a,
        b,
        value: 0.0,
        expr: None,
        aligned: false,
        suppressed: false,
        locked: false,
        min: None,
        max: None,
        resting: false,
        plane: None,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "assemble_demo.3dcadcraft".into());
    let mut project = Project::new("Assemble demo");
    let plate = project.add_document(
        "Plate",
        DocumentData::Part(PartDocument::with_box("Box1", [120.0, 80.0, 10.0])?),
    );
    let block = project.add_document(
        "Block",
        DocumentData::Part(PartDocument::with_box("Box1", [30.0, 20.0, 15.0])?),
    );
    let pin_id = project.add_document("Pin", DocumentData::Part(pin()?));

    let mut a = AssemblyDocument::default();
    let plate_occ = a.place(DocRef::local(plate), "Plate", at(0.0, 0.0, 0.0), true);
    let block_1 = a.place(DocRef::local(block), "Block", at(10.0, 10.0, 40.0), false);
    let block_2 = a.place(DocRef::local(block), "Block", at(70.0, 40.0, 60.0), false);
    a.place(DocRef::local(pin_id), "Pin", at(-40.0, 20.0, 30.0), false);
    a.place(DocRef::local(pin_id), "Pin", at(-40.0, 60.0, 30.0), false);
    let origin = |occurrence, item| GeomRef::Origin {
        sub: Vec::new(),
        occurrence,
        item,
    };
    // Block:1 rests on the plate's plane: two slides and a turn are left.
    a.add_constraint(relation(
        ConstraintKind::Flush,
        origin(plate_occ, OriginItem::XyPlane),
        origin(block_1, OriginItem::XyPlane),
    ));
    // Block:2 keeps a relationship with a face that no longer exists.
    a.add_constraint(relation(
        ConstraintKind::Mate,
        GeomRef::Face {
            sub: Vec::new(),
            occurrence: block_2,
            name: "Gone:Top".into(),
            hint: [0.0; 3],
        },
        origin(plate_occ, OriginItem::XyPlane),
    ));
    project.add_document("Assembly", DocumentData::Assembly(a));
    project.save(std::path::Path::new(&path))?;
    println!("saved {path}");
    Ok(())
}
