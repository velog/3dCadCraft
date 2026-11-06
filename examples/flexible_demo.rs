//! A flexible subassembly to try (`docs/spec/3.1a-subassemblies.md`, S5): a
//! "Slide" assembly — a grounded 100 × 20 × 10 rail and a free 20 × 20 × 10
//! carriage on it (Flush of their XY and XZ planes: it slides along X, turns
//! about Z) — placed twice in "Top" next to a grounded Stop block at the rail's end (x 100).
//! Top mates the first Slide's carriage end with the Stop's face (Flush of
//! YZ planes, 20 apart). While Slide:1 is rigid that cannot hold; right-click
//! Slide:1 › Flexible and its carriage slides to the rail's end against the Stop
//! (x 80), Slide:2's stays at x 10.
//!
//! `cargo run --example flexible_demo -- out.3dcadcraft`

use cadcraft3d::assembly::relation::{Constraint, ConstraintKind, GeomRef, OriginItem};
use cadcraft3d::document::{AssemblyDocument, DocRef, DocumentData, Project};
use cadcraft3d::kernel::{Transform3, Vector3};
use cadcraft3d::part::PartDocument;

fn origin(occurrence: u32, sub: Vec<u32>, item: OriginItem) -> GeomRef {
    GeomRef::Origin {
        occurrence,
        sub,
        item,
    }
}

fn push(a: &mut AssemblyDocument, name: &str, kind: ConstraintKind, x: GeomRef, y: GeomRef) {
    let mut c = Constraint::new(kind, x, y);
    c.id = a.constraints.iter().map(|c| c.id).max().unwrap_or(0) + 1;
    c.name = format!("{name}:{}", c.id);
    a.constraints.push(c);
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "flexible_demo.3dcadcraft".into());
    let mut project = Project::new("FlexibleDemo");
    let rail = project.add_document(
        "Rail",
        DocumentData::Part(PartDocument::with_box("Box1", [100.0, 20.0, 10.0])?),
    );
    let carriage = project.add_document(
        "Carriage",
        DocumentData::Part(PartDocument::with_box("Box1", [20.0, 20.0, 10.0])?),
    );
    let stop = project.add_document(
        "Stop",
        DocumentData::Part(PartDocument::with_box("Box1", [10.0, 20.0, 30.0])?),
    );
    let mut slide = AssemblyDocument::default();
    let r = slide.place(DocRef::local(rail), "Rail", Transform3::identity(), true);
    let c = slide.place(
        DocRef::local(carriage),
        "Carriage",
        Transform3::translation(10.0, 0.0, 10.0),
        false,
    );
    // The carriage stands on the rail (its XY plane 10 mm up) and keeps its side.
    let mut on = Constraint::new(
        ConstraintKind::Flush,
        origin(r, Vec::new(), OriginItem::XyPlane),
        origin(c, Vec::new(), OriginItem::XyPlane),
    );
    on.id = 1;
    on.name = "Flush:1".into();
    on.value = 10.0;
    slide.constraints.push(on);
    push(
        &mut slide,
        "Flush",
        ConstraintKind::Flush,
        origin(r, Vec::new(), OriginItem::XzPlane),
        origin(c, Vec::new(), OriginItem::XzPlane),
    );
    let slide = project.add_document("Slide", DocumentData::Assembly(slide));
    let mut top = AssemblyDocument::default();
    let s1 = top.place(DocRef::local(slide), "Slide", Transform3::identity(), true);
    top.place(
        DocRef::local(slide),
        "Slide",
        Transform3::new(Vector3::new(0.0, 60.0, 0.0), Vector3::zeros()),
        true,
    );
    let st = top.place(
        DocRef::local(stop),
        "Stop",
        Transform3::translation(100.0, 0.0, 0.0),
        true,
    );
    // The first Slide's carriage against the Stop (it is 20 long: its YZ
    // plane 20 mm before the Stop's).
    push(
        &mut top,
        "Flush",
        ConstraintKind::Flush,
        origin(s1, vec![c], OriginItem::YzPlane),
        origin(st, Vec::new(), OriginItem::YzPlane),
    );
    if let Some(k) = top.constraints.last_mut() {
        k.value = 20.0;
    }
    project.add_document("Top", DocumentData::Assembly(top));
    project.save(std::path::Path::new(&path))?;
    println!("saved {path}");
    Ok(())
}
