//! Writes an assembly for trying Smart Drop (spec 6.3 §3.5): a plate with four
//! clearance holes of one feature, two tapped holes and a counterbore, and a
//! shaft beside it. `cargo run --example smart_drop_demo -- out.3dcadcraft`

use cadcraft3d::components::samples::{Detail, Kind, Sample};
use cadcraft3d::document::{AssemblyDocument, DocRef, DocumentData, Project};
use cadcraft3d::kernel::{Point3, Real, Transform3};
use cadcraft3d::part::PartDocument;
use cadcraft3d::part::hole::{HoleCenter, HoleFeature, HoleSpec, ThreadSpec};
use cadcraft3d::part::standards::{ISO_METRIC, Standards};

const TOP: &str = "Box1:Top";

fn holes(part: &mut PartDocument, spec: HoleSpec, at: &[(Real, Real)]) -> Result<(), String> {
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
    let mut def = HoleFeature::new(spec);
    def.sketch = part.sketches[i].id.clone();
    def.centers = ids.into_iter().map(HoleCenter::Point).collect();
    part.add_hole(def).map_err(|e| e.to_string())?;
    Ok(())
}

fn main() -> Result<(), String> {
    let out = std::env::args()
        .nth(1)
        .ok_or("usage: smart_drop_demo out.3dcadcraft")?;
    let mut plate =
        PartDocument::with_box("Box1", [120.0, 80.0, 12.0]).map_err(|e| e.to_string())?;
    // Four clearance holes of one feature: Follow Pattern.
    let mut through = HoleSpec::simple(9.0, 12.0);
    through.termination = cadcraft3d::part::hole::Termination::ThroughAll;
    holes(
        &mut plate,
        through,
        &[(15.0, 15.0), (45.0, 15.0), (15.0, 65.0), (45.0, 65.0)],
    )?;
    // Two tapped blind holes: the thread decides, not the drill.
    let table = Standards::builtin();
    let m8 = ThreadSpec::from_table(&table, ISO_METRIC, "M8", true).ok_or("no M8 in the table")?;
    holes(
        &mut plate,
        HoleSpec::tapped(m8, 10.0),
        &[(75.0, 65.0), (105.0, 65.0)],
    )?;
    // A housing for a 6204 (outside 47).
    let mut housing = HoleSpec::simple(47.0, 12.0);
    housing.termination = cadcraft3d::part::hole::Termination::ThroughAll;
    holes(&mut plate, housing, &[(90.0, 25.0)])?;

    // A hole a 20 mm shaft fits, for a part with its own connection.
    let mut bore = HoleSpec::simple(20.0, 12.0);
    bore.termination = cadcraft3d::part::hole::Termination::ThroughAll;
    holes(&mut plate, bore, &[(60.0, 45.0)])?;

    let shaft = Sample {
        category: String::new(),
        family: "Shaft".into(),
        standard: "Other".into(),
        size: "20 x 80".into(),
        kind: Kind::Pin(20.0, 80.0),
        finish: None,
    }
    .part_at(Detail::Standard)
    .map_err(|e| e.to_string())?;

    let mut project = Project::new("SmartDropDemo");
    let plate_doc = project.add_document("Plate", DocumentData::Part(plate));
    let shaft_doc = project.add_document("Shaft", DocumentData::Part(shaft));
    let mut a = AssemblyDocument::default();
    a.place(
        DocRef::local(plate_doc),
        "Plate",
        Transform3::identity(),
        true,
    );
    a.place(
        DocRef::local(shaft_doc),
        "Shaft",
        Transform3::translation(150.0, 40.0, 0.0),
        true,
    );
    project.add_document("Top", DocumentData::Assembly(a));
    project
        .save(std::path::Path::new(&out))
        .map_err(|e| e.to_string())?;
    println!("{out}");
    Ok(())
}
