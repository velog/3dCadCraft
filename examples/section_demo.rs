//! A plate with two tapped M8 holes, two Ø8 pins standing in them and a
//! drawing of the assembly with a section through both pins, for trying
//! section participation (spec 4.15), Thread Feature and the symbols of the
//! Symbols panel. The first pin's occurrence is set not to be cut; the
//! positional representation "Lifted" raises the second pin (Overlay View).
//!
//! ```sh
//! cargo run --example section_demo -- /tmp/section.3dcadcraft
//! cargo run -- /tmp/section.3dcadcraft      # open it, then the drawing
//! ```

use std::path::PathBuf;

use cadcraft3d::document::iprops::SectionParticipation;
use cadcraft3d::document::{AssemblyDocument, DocRef, DocumentData, Project};
use cadcraft3d::drawing::{DrawingDocument, ViewOrientation, ViewStyle};
use cadcraft3d::kernel::{Point3, Transform3};
use cadcraft3d::part::hole::HoleCenter;
use cadcraft3d::part::standards::{self, Standards};
use cadcraft3d::part::{
    HoleFeature, HoleSpec, OriginPlane, PartDocument, PrimitiveFeature, PrimitiveKind,
    SketchPlacement, ThreadSpec,
};
use cadcraft3d::sketch::Point2;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path: PathBuf = std::env::args_os()
        .nth(1)
        .map_or("section.3dcadcraft".into(), Into::into);
    // Plate 80 × 40 × 10, tapped M8 at (20, 20) and (60, 20).
    let mut plate = PartDocument::with_box("Box1", [80.0, 40.0, 10.0])?;
    let m8 = ThreadSpec::from_table(&Standards::builtin(), standards::ISO_METRIC, "M8", true)
        .ok_or("M8")?;
    let pts = [Point3::new(20.0, 20.0, 10.0), Point3::new(60.0, 20.0, 10.0)];
    let (i, ids) = plate.create_hole_sketch("Box1:Top", &pts)?;
    let mut hole = HoleFeature::new(HoleSpec::tapped(m8, 15.0));
    hole.sketch = plate.sketches[i].id.clone();
    hole.centers = ids.into_iter().map(HoleCenter::Point).collect();
    plate.add_hole(hole)?;
    // A Ø8 pin 30 long.
    let mut pin = PartDocument::default();
    let f = PrimitiveFeature::extrude(&pin, 30.0);
    cadcraft3d::part::primitive::add_primitive(
        &mut pin,
        PrimitiveKind::Cylinder,
        SketchPlacement::on_origin_plane(OriginPlane::XY),
        &[Point2::new(0.0, 0.0), Point2::new(4.0, 0.0)],
        f,
    )?;

    let mut project = Project::new("Section");
    let plate_id = project.add_document("Plate", DocumentData::Part(plate));
    let pin_id = project.add_document("Pin", DocumentData::Part(pin));
    let mut asm = AssemblyDocument::default();
    asm.place(
        DocRef::local(plate_id),
        "Plate",
        Transform3::identity(),
        true,
    );
    let first = asm.place(
        DocRef::local(pin_id),
        "Pin",
        Transform3::translation(20.0, 20.0, -10.0),
        false,
    );
    asm.place(
        DocRef::local(pin_id),
        "Pin",
        Transform3::translation(60.0, 20.0, -10.0),
        false,
    );
    if let Some(o) = asm.find_mut(first) {
        o.section = SectionParticipation::None;
    }
    // A positional representation "Lifted" with the second pin raised 15 mm
    // (for Overlay View).
    let second = asm.occurrences.last().map_or(0, |o| o.id);
    let mut reps = std::mem::take(&mut asm.reps);
    reps.new_position(&mut asm);
    if let Some(p) = reps.positions.last_mut() {
        p.name = "Lifted".into();
        p.poses
            .insert(second, Transform3::translation(60.0, 20.0, 5.0));
    }
    // The assembly itself stands in [Primary] (its occurrences hold the
    // active representation's places).
    reps.active_position = cadcraft3d::assembly::reps::PRIMARY;
    asm.reps = reps;
    let asm_id = project.add_document("Assembly", DocumentData::Assembly(asm));
    let mut d = DrawingDocument::default();
    let base = d
        .add_base_view(
            DocRef::local(asm_id),
            ViewOrientation::Top,
            [150.0, 210.0],
            1.5,
            ViewStyle::HiddenRemoved,
        )
        .ok_or("base view")?;
    d.add_section_view(base, [-10.0, 20.0], [90.0, 20.0], [150.0, 110.0])
        .ok_or("section view")?;
    project.add_document("Drawing", DocumentData::Drawing(d));
    project.save(&path)?;
    println!("{}", path.display());
    Ok(())
}
