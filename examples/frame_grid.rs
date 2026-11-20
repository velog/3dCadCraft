//! A frame assembly for view-speed checks (`docs/PERF.md`, hover while orbiting): `n × n`
//! skeleton boxes (400 × 300 × 200, 50 mm apart) and a square hollow section 40x40x3 on
//! each of their twelve edges, mitred at the corners — `12 n²` member parts with rounded
//! sections.
//!
//! `cargo run --example frame_grid -- out.3dcadcraft 3`

use cadcraft3d::assembly::frame::{self, Corners, InsertOptions, Layout, Orientation, Skeleton};
use cadcraft3d::assembly::relation::GeomRef;
use cadcraft3d::assembly::tree::Linked;
use cadcraft3d::document::{AssemblyDocument, DocRef, DocumentData, Project};
use cadcraft3d::kernel::{Real, Transform3, Vector3};
use cadcraft3d::part::frame::Library;
use cadcraft3d::part::{EdgeRef, PartDocument};

/// The twelve edges of a box `[x, y, z]` with its corner at the origin: hint and faces.
fn box_edges(s: [Real; 3]) -> Vec<([Real; 3], &'static str, &'static str)> {
    let [x, y, z] = s;
    let (hx, hy, hz) = (x / 2.0, y / 2.0, z / 2.0);
    vec![
        ([hx, 0.0, z], "Front", "Top"),
        ([hx, y, z], "Back", "Top"),
        ([0.0, hy, z], "Left", "Top"),
        ([x, hy, z], "Right", "Top"),
        ([hx, 0.0, 0.0], "Front", "Bottom"),
        ([hx, y, 0.0], "Back", "Bottom"),
        ([0.0, hy, 0.0], "Left", "Bottom"),
        ([x, hy, 0.0], "Right", "Bottom"),
        ([0.0, 0.0, hz], "Front", "Left"),
        ([x, 0.0, hz], "Front", "Right"),
        ([0.0, y, hz], "Back", "Left"),
        ([x, y, hz], "Back", "Right"),
    ]
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = args
        .first()
        .cloned()
        .unwrap_or_else(|| "frame_grid.3dcadcraft".into());
    let n: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(3);
    let size = [400.0, 300.0, 200.0];
    let mut p = Project::new("FrameGrid");
    let skeleton = PartDocument::with_box("Box1", size)?;
    let sk = p.add_document("Skeleton", DocumentData::Part(skeleton));
    let mut a = AssemblyDocument::default();
    let mut occs = Vec::new();
    for i in 0..n {
        for j in 0..n {
            let at = Vector3::new(i as Real * 450.0, j as Real * 350.0, 0.0);
            let id = a.place(
                DocRef::local(sk),
                "Skeleton",
                Transform3::from_parts(at.into(), Default::default()),
                true,
            );
            occs.push(id);
        }
    }
    for o in a.occurrences.iter_mut() {
        o.visible = false;
    }
    let top = p.add_document("Frame", DocumentData::Assembly(a));
    let linked = Linked::new();
    let opts = InsertOptions {
        family: "Square hollow section (EN 10219)".into(),
        size: "40x40x3".into(),
        material: Some("Steel S235".into()),
        orientation: Orientation::default(),
        corners: Corners::default(),
        layout: Layout::default(),
        appearance: None,
        appearance_to_part: false,
    };
    let t = std::time::Instant::now();
    let mut members = 0;
    for occ in occs {
        let edges: Vec<Skeleton> = box_edges(size)
            .into_iter()
            .map(|(hint, f0, f1)| {
                Skeleton::Edge(GeomRef::Edge {
                    occurrence: occ,
                    sub: Vec::new(),
                    edge: EdgeRef {
                        faces: [format!("Box1:{f0}"), format!("Box1:{f1}")],
                        hint,
                    },
                })
            })
            .collect();
        let (_, made) = frame::insert(
            &mut p,
            &linked,
            top,
            &edges,
            &opts,
            Library::builtin(),
            None,
        )?;
        members += made.len();
    }
    p.save(std::path::Path::new(&path))?;
    println!(
        "saved {path}: {members} members in {:.1} s",
        t.elapsed().as_secs_f64()
    );
    Ok(())
}
