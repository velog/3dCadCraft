//! A small assembly to try the Frame Generator from its parent: a skeleton
//! box (400 × 300 × 200) and a frame subassembly with the four top edges as
//! 40×40×3 SHS members, corners uncut (their joints show amber icons) — for
//! the joint icons, Edit Frame Members and Pattern Component of members.
//!
//! `cargo run --example frame_demo -- out.3dcadcraft`

use cadcraft3d::assembly::frame::{self, Corners, InsertOptions, Layout, Orientation, Skeleton};
use cadcraft3d::assembly::frame_sub::{self, Target};
use cadcraft3d::assembly::relation::GeomRef;
use cadcraft3d::assembly::tree::Linked;
use cadcraft3d::document::{AssemblyDocument, DocRef, DocumentData, Project};
use cadcraft3d::kernel::{Real, Transform3};
use cadcraft3d::part::frame::Library;
use cadcraft3d::part::{EdgeRef, PartDocument};

fn edge(hint: [Real; 3], f0: &str, f1: &str) -> Skeleton {
    Skeleton::Edge(GeomRef::Edge {
        occurrence: 1,
        sub: Vec::new(),
        edge: EdgeRef {
            faces: [format!("Box1:{f0}"), format!("Box1:{f1}")],
            hint,
        },
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "frame_demo.3dcadcraft".into());
    let mut p = Project::new("Frame demo");
    let sk = PartDocument::with_box("Box1", [400.0, 300.0, 200.0])?;
    let sk = p.add_document("Skeleton", DocumentData::Part(sk));
    let mut a = AssemblyDocument::default();
    a.place(
        DocRef::local(sk),
        "Skeleton:1",
        Transform3::identity(),
        true,
    );
    let top = p.add_document("Top", DocumentData::Assembly(a));
    let linked = Linked::new();
    let (frame, doc) = frame_sub::new_frame_assembly(&mut p, top, "Frame0001")?;
    let t = Target {
        top,
        frame,
        doc,
        skeleton: "Skeleton0001",
    };
    let lines = [
        edge([200.0, 0.0, 200.0], "Front", "Top"),
        edge([400.0, 150.0, 200.0], "Right", "Top"),
        edge([200.0, 300.0, 200.0], "Back", "Top"),
        edge([0.0, 150.0, 200.0], "Left", "Top"),
    ];
    let down: Vec<Skeleton> = lines
        .iter()
        .map(|s| frame_sub::skeleton_down(&mut p, &linked, t, s))
        .collect::<Result<_, _>>()?;
    let o = InsertOptions {
        family: "Square hollow section (EN 10219)".into(),
        size: "40x40x3".into(),
        material: Some("Steel S235".into()),
        orientation: Orientation::default(),
        corners: Corners::default(),
        layout: Layout::default(),
        appearance: None,
        appearance_to_part: false,
    };
    frame::insert(&mut p, &linked, doc, &down, &o, Library::builtin(), None)?;
    p.save(std::path::Path::new(&out))?;
    println!("{out}");
    Ok(())
}
