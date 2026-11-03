//! An assembly to try Variants (spec 3.9): a bolt part with the variants `[Primary]` and
//! `Long` (other length, material and Part Number), placed twice in one assembly with a
//! different variant each; a plate; a circular pattern of bolts; a relationship with a value.
//! The assembly's variant `Eight` has 8 bolts in the pattern and another relationship value.
//!
//! `cargo run --example variants_demo -- out.3dcadcraft`

use cadcraft3d::assembly::pattern::{ComponentPattern, Layout};
use cadcraft3d::assembly::relation::{Constraint, ConstraintKind, GeomRef, OriginItem};
use cadcraft3d::document::{AssemblyDocument, DocRef, DocumentData, Project};
use cadcraft3d::kernel::{Real, Transform3, Vector3};
use cadcraft3d::part::variants::RowKey;
use cadcraft3d::part::variants::{Cell, PRIMARY};
use cadcraft3d::part::{
    ExtrudeFeature, OriginPlane, PartDocument, ProfileRef, SketchOptions, SketchPlacement,
};
use cadcraft3d::sketch::Point2;

fn bolt() -> Result<PartDocument, Box<dyn std::error::Error>> {
    let mut part = PartDocument::default();
    let opts = SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    };
    let i = part.create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts)?;
    part.sketches[i]
        .sketch
        .add_circle(Point2::new(0.0, 0.0), 4.0)?;
    let regions = part.sketches[i].sketch.regions();
    part.add_extrude(
        i,
        ExtrudeFeature::new(
            vec![ProfileRef::of(&regions[0], regions[0].interior_point())],
            20.0,
        ),
    )?;
    Ok(part)
}

fn at(x: Real, y: Real, z: Real) -> Transform3 {
    Transform3::new(Vector3::new(x, y, z), Vector3::zeros())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "variants_demo.3dcadcraft".into());
    let mut project = Project::new("Variants demo");

    let mut b = bolt()?;
    let length = b
        .parameter_table()
        .first()
        .map(|r| r.name.clone())
        .ok_or("the bolt has no parameter")?;
    let long = b.new_variant();
    if let Some(v) = b.variants.list.iter_mut().find(|v| v.id == long) {
        v.name = "Long".into();
    }
    b.set_parameter_expr(&length, "45 mm")?;
    b.variants_sync();
    let steel = cadcraft3d::materials::with_user()
        .materials
        .iter()
        .map(|m| m.name.clone())
        .find(|n| n != "Generic")
        .ok_or("no material")?;
    b.set_variant_info(long, &RowKey::Material, &Cell::Text(steel))?;
    b.set_variant_info(
        long,
        &RowKey::Prop("part_number".into()),
        &Cell::Text("BOLT-45".into()),
    )?;
    b.variants_sync();
    b.activate_variant(PRIMARY)?;

    let bolt_id = project.add_document("Bolt", DocumentData::Part(b));
    if let Some(d) = project.document_mut(bolt_id) {
        d.properties.set("part_number", "BOLT-20");
    }
    let plate_id = project.add_document(
        "Plate",
        DocumentData::Part(PartDocument::with_box("Box1", [120.0, 120.0, 8.0])?),
    );

    let mut a = AssemblyDocument::default();
    let bolt_ref = DocRef::local(bolt_id);
    a.place(
        DocRef::local(plate_id),
        "Plate",
        at(-60.0, -60.0, 0.0),
        true,
    );
    // One part, two variants, side by side.
    a.place(bolt_ref, "Bolt", at(-20.0, 0.0, 8.0), false);
    let long_ref = DocRef {
        variant: Some(long),
        ..bolt_ref
    };
    a.place(long_ref, "Bolt", at(20.0, 0.0, 8.0), false);
    // A bolt on a circle, patterned about the assembly's Z axis.
    let ringed = a.place(bolt_ref, "Bolt", at(45.0, 0.0, 8.0), true);
    let z_axis = GeomRef::Origin {
        sub: Vec::new(),
        occurrence: 0,
        item: OriginItem::ZAxis,
    };
    a.patterns.push(ComponentPattern {
        id: 1,
        name: "Component Pattern 1".into(),
        sources: vec![ringed],
        layout: Layout::Circular {
            axis: z_axis,
            flip: false,
            count: 4,
            angle: 90f64.to_radians(),
        },
        elements: Vec::new(),
    });
    // Bolt:2 stands at a distance from the plate's top.
    let origin = |occurrence, item| GeomRef::Origin {
        sub: Vec::new(),
        occurrence,
        item,
    };
    a.add_constraint(Constraint {
        id: 0,
        name: String::new(),
        kind: ConstraintKind::Flush,
        a: origin(1, OriginItem::XyPlane),
        b: origin(2, OriginItem::XyPlane),
        value: 8.0,
        expr: None,
        aligned: false,
        suppressed: false,
        locked: false,
        min: None,
        max: None,
        resting: false,
        plane: None,
    });

    // The assembly's own variant: a bigger circle of bolts, Bolt:2 higher.
    let mut reps = std::mem::take(&mut a.reps);
    let eight = reps.new_variant(&a);
    // Not the active one when it is edited: the active variant follows the assembly.
    reps.activate_variant(PRIMARY, &mut a);
    a.reps = reps;
    if let Some(v) = a.reps.variants.iter_mut().find(|v| v.id == eight) {
        v.name = "Eight".into();
        v.values.entry(1).or_default().value = 30.0;
        if let Some(Layout::Circular { count, angle, .. }) = v.patterns.get_mut(&1) {
            *count = 8;
            *angle = 45f64.to_radians();
        }
        v.info.set("part_number", "ASM-8");
    }

    project.add_document("Assembly", DocumentData::Assembly(a));
    project.save(std::path::Path::new(&path))?;
    println!("saved {path}");
    Ok(())
}
