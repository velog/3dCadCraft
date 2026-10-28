//! A design file with a subassembly (`docs/spec/3.1a-subassemblies.md`): a
//! "Bracket" assembly (a 40 × 30 × 5 plate with two 8 × 8 × 20 posts) and a
//! "Top" assembly with a 120 × 80 × 6 base plate and the Bracket placed
//! twice, the second one turned 90°.
//!
//! `cargo run --example subassembly_demo -- out.3dcadcraft [overlap]`
//! (`overlap`: the second Bracket stands in the first one, for Analyze
//! Interference).

use cadcraft3d::document::{AssemblyDocument, DocRef, DocumentData, Project};
use cadcraft3d::kernel::{Transform3, Vector3};
use cadcraft3d::part::PartDocument;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "subassembly_demo.3dcadcraft".into());
    let overlap = std::env::args().nth(2).as_deref() == Some("overlap");
    let mut project = Project::new("SubassemblyDemo");
    let base = project.add_document(
        "Base",
        DocumentData::Part(PartDocument::with_box("Box1", [120.0, 80.0, 6.0])?),
    );
    let plate = project.add_document(
        "BracketPlate",
        DocumentData::Part(PartDocument::with_box("Box1", [40.0, 30.0, 5.0])?),
    );
    let post = project.add_document(
        "Post",
        DocumentData::Part(PartDocument::with_box("Box1", [8.0, 8.0, 20.0])?),
    );
    let mut bracket = AssemblyDocument::default();
    bracket.place(
        DocRef::local(plate),
        "BracketPlate",
        Transform3::identity(),
        true,
    );
    for x in [4.0, 28.0] {
        let t = Transform3::translation(x, 11.0, 5.0);
        bracket.place(DocRef::local(post), "Post", t, false);
    }
    let bracket = project.add_document("Bracket", DocumentData::Assembly(bracket));
    let mut top = AssemblyDocument::default();
    top.place(DocRef::local(base), "Base", Transform3::identity(), true);
    top.place(
        DocRef::local(bracket),
        "Bracket",
        Transform3::translation(10.0, 10.0, 6.0),
        false,
    );
    top.place(
        DocRef::local(bracket),
        "Bracket",
        Transform3::new(
            Vector3::new(if overlap { 30.0 } else { 100.0 }, 20.0, 6.0),
            Vector3::z() * std::f64::consts::FRAC_PI_2,
        ),
        false,
    );
    project.add_document("Top", DocumentData::Assembly(top));
    project.save(std::path::Path::new(&path))?;
    println!("saved {path}");
    Ok(())
}
