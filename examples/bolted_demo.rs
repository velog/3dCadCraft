//! Writes an assembly for trying Bolted Connection (spec 6.2): two 10 mm
//! plates stacked (the top one with four Ø9 holes of one Hole feature, for By
//! Hole and Follow Pattern), and a 10 mm plate on a 40 mm block beside them
//! (Blind), and two 12 mm flanges with a Ø50 bore behind them (Circular).
//! `cargo run --example bolted_demo -- out.3dcadcraft`

use cadcraft3d::document::{AssemblyDocument, DocRef, DocumentData, Project};
use cadcraft3d::kernel::{Point3, Real, Transform3};
use cadcraft3d::part::PartDocument;
use cadcraft3d::part::hole::{HoleCenter, HoleFeature, HoleSpec, Termination};

const TOP: &str = "Box1:Top";

fn holes(part: &mut PartDocument, at: &[(Real, Real)], dia: Real) -> Result<(), String> {
    let z = part
        .body
        .faces()
        .find(|(_, f)| f.name.as_deref() == Some(TOP))
        .and_then(|(_, f)| f.mesh.as_ref().map(|m| m.positions[0].z))
        .ok_or("the plate has no top")?;
    let pts: Vec<Point3> = at.iter().map(|&(x, y)| Point3::new(x, y, z)).collect();
    let (i, ids) = part
        .create_hole_sketch(TOP, &pts)
        .map_err(|e| e.to_string())?;
    let mut spec = HoleSpec::simple(dia, 10.0);
    spec.termination = Termination::ThroughAll;
    let mut def = HoleFeature::new(spec);
    def.sketch = part.sketches[i].id.clone();
    def.centers = ids.into_iter().map(HoleCenter::Point).collect();
    part.add_hole(def).map_err(|e| e.to_string())?;
    Ok(())
}

fn main() -> Result<(), String> {
    let out = std::env::args()
        .nth(1)
        .ok_or("usage: bolted_demo out.3dcadcraft")?;
    let plate = PartDocument::with_box("Box1", [120.0, 80.0, 10.0]).map_err(|e| e.to_string())?;
    let mut holed = plate.clone();
    holes(
        &mut holed,
        &[(15.0, 15.0), (105.0, 15.0), (15.0, 65.0), (105.0, 65.0)],
        9.0,
    )?;
    let mut flange =
        PartDocument::with_box("Box1", [140.0, 140.0, 12.0]).map_err(|e| e.to_string())?;
    holes(&mut flange, &[(70.0, 70.0)], 50.0)?;
    let block = PartDocument::with_box("Box1", [120.0, 80.0, 40.0]).map_err(|e| e.to_string())?;

    let mut project = Project::new("BoltedDemo");
    let plate_doc = project.add_document("Plate", DocumentData::Part(plate));
    let holed_doc = project.add_document("Holed Plate", DocumentData::Part(holed));
    let block_doc = project.add_document("Block", DocumentData::Part(block));
    let flange_doc = project.add_document("Flange", DocumentData::Part(flange));
    let mut a = AssemblyDocument::default();
    let at = |x: Real, z: Real| Transform3::translation(x, 0.0, z);
    a.place(DocRef::local(plate_doc), "Plate", at(0.0, 0.0), true);
    a.place(DocRef::local(holed_doc), "Holed Plate", at(0.0, 10.0), true);
    a.place(DocRef::local(block_doc), "Block", at(160.0, 0.0), true);
    a.place(DocRef::local(plate_doc), "Plate", at(160.0, 40.0), true);
    let fl = |z: Real| Transform3::translation(0.0, 120.0, z);
    a.place(DocRef::local(flange_doc), "Flange", fl(0.0), true);
    a.place(DocRef::local(flange_doc), "Flange", fl(12.0), true);
    project.add_document("Top", DocumentData::Assembly(a));
    project
        .save(std::path::Path::new(&out))
        .map_err(|e| e.to_string())?;
    println!("{out}");
    Ok(())
}
