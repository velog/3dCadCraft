//! Mechanisms to drag (`docs/spec/3.1a-subassemblies.md` S5): "Arm" (a turn,
//! a slide along the arm, a second turn) and "Delta" (a delta robot: three
//! hinged arms, rods on Balls to one effector), each a subassembly placed
//! flexible in "Top" — drag the effector or an arm's tip in Top and the
//! parts inside follow their joints.
//!
//! `cargo run --example mechanisms_demo -- out.3dcadcraft`

use cadcraft3d::assembly::mechanisms;
use cadcraft3d::document::{AssemblyDocument, DocRef, DocumentData, Project};
use cadcraft3d::kernel::Transform3;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "mechanisms_demo.3dcadcraft".into());
    let mut project = Project::new("Mechanisms");
    let arm = mechanisms::arm(&mut project)?;
    let delta = mechanisms::delta(&mut project)?;
    let mut top = AssemblyDocument::default();
    let d = top.place(
        DocRef::local(delta.doc),
        "Delta",
        Transform3::translation(0.0, 0.0, 200.0),
        true,
    );
    let a = top.place(
        DocRef::local(arm.doc),
        "Arm",
        Transform3::translation(250.0, 0.0, 0.0),
        true,
    );
    for id in [d, a] {
        if let Some(o) = top.find_mut(id) {
            o.flexible = true;
        }
    }
    project.add_document("Top", DocumentData::Assembly(top));
    project.save(std::path::Path::new(&path))?;
    println!("saved {path}");
    Ok(())
}
