//! A welded frame (spec 6.1): a 900 × 500 × 700 box as the skeleton, one
//! 50 × 50 × 4 square hollow section along each of its twelve edges, the
//! corners treated, the skeleton hidden.
//!
//! `cargo run --example frame_demo -- out.3dcadcraft`

use cadcraft3d::assembly::frame::{CornerType, Corners, InsertOptions, Skeleton, insert};
use cadcraft3d::assembly::relation::GeomRef;
use cadcraft3d::assembly::tree::Linked;
use cadcraft3d::document::{AssemblyDocument, DocRef, DocumentData, Project};
use cadcraft3d::kernel::{TessellationOptions, Transform3};
use cadcraft3d::part::frame::Library;
use cadcraft3d::part::{EdgeRef, PartDocument};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: frame_demo out.3dcadcraft")?;
    let skeleton = PartDocument::with_box("Box1", [900.0, 500.0, 700.0])?;
    let mut edges = Vec::new();
    for (id, _) in skeleton.body.edges() {
        let pts = skeleton
            .body
            .sample_edge(id, &TessellationOptions::default())?;
        let mid = pts[0] + (pts[pts.len() - 1] - pts[0]) * 0.5;
        let faces = skeleton.body.edge_faces(id);
        let name = |k: usize| {
            skeleton
                .body
                .face(faces[k])
                .and_then(|f| f.name.clone())
                .unwrap_or_default()
        };
        edges.push(Skeleton::Edge(GeomRef::Edge {
            occurrence: 1,
            sub: Vec::new(),
            edge: EdgeRef {
                faces: [name(0), name(1)],
                hint: [mid.x, mid.y, mid.z],
            },
        }));
    }
    let mut project = Project::new("Frame");
    let part = project.add_document("Skeleton", DocumentData::Part(skeleton));
    let mut assembly = AssemblyDocument::default();
    assembly.place(
        DocRef::local(part),
        "Skeleton:1",
        Transform3::identity(),
        true,
    );
    let frame = project.add_document("Frame", DocumentData::Assembly(assembly));
    let options = InsertOptions {
        family: "Square hollow section (EN 10219)".into(),
        size: "50x50x4".into(),
        material: Some("Steel S235".into()),
        orientation: Default::default(),
        corners: Corners {
            kind: CornerType::Miter,
            ..Default::default()
        },
        layout: Default::default(),
    };
    insert(
        &mut project,
        &Linked::new(),
        frame,
        &edges,
        &options,
        Library::builtin(),
        None,
    )
    .map_err(|e| format!("Insert Frame: {e}"))?;
    if let Some(DocumentData::Assembly(a)) = project.document_mut(frame).map(|d| &mut d.data)
        && let Some(o) = a.occurrences.iter_mut().find(|o| o.id == 1)
    {
        o.visible = false;
    }
    project.save(std::path::Path::new(&path))?;
    println!("saved {path}");
    Ok(())
}
