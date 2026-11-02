//! What a saved part shows when another document of the file is opened first: display triangles
//! are not saved, and a free-form face has no tessellation of its own (owner's scissors, whose
//! blade opened with a hole in its spline wall, the back faces behind it red).

use cadcraft3d::document::{DocumentData, Project};
use cadcraft3d::part::*;

fn parts(p: &Project) -> Vec<&PartDocument> {
    p.documents()
        .iter()
        .filter_map(|d| match &d.data {
            DocumentData::Part(x) => Some(x),
            _ => None,
        })
        .collect()
}

#[test]
fn a_spline_wall_is_drawn_after_loading_and_a_plain_part_is_left_alone() {
    // The owner's blade as its file saved it (chamfers, rounds and a hole on a spline outline).
    let blade =
        Project::load(std::path::Path::new("tests/data/scissors_blade.3dcadcraft")).unwrap();
    let blade = parts(&blade)[0].clone();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("two.3dcadcraft");
    let mut project = Project::new("Two");
    project.add_document("Blade", DocumentData::Part(blade));
    project.add_document(
        "Block",
        DocumentData::Part(PartDocument::with_box("Box1", [10.0, 10.0, 10.0]).unwrap()),
    );
    project.save(&path).unwrap();

    let mut back = Project::load(&path).unwrap();
    assert!(
        parts(&back)[0].has_undrawable_faces(),
        "the case this guards"
    );
    assert!(!parts(&back)[1].has_undrawable_faces());
    assert_eq!(back.rebuild_unmeshed_parts(), 1);
    assert!(parts(&back).iter().all(|p| !p.has_undrawable_faces()));
    assert!(parts(&back)[0].feature_health.is_empty());
}

/// Asking every part costs a tessellation each: opening a file of many parts must stay quick.
#[test]
fn asking_every_part_stays_quick() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("many.3dcadcraft");
    let mut project = Project::new("Many");
    for k in 0..24 {
        project.add_document(
            format!("Block{k}"),
            DocumentData::Part(PartDocument::with_box("Box1", [10.0, 10.0, 10.0]).unwrap()),
        );
    }
    project.save(&path).unwrap();
    let mut back = Project::load(&path).unwrap();
    let t = std::time::Instant::now();
    assert_eq!(back.rebuild_unmeshed_parts(), 0);
    let ms = t.elapsed().as_secs_f64() * 1e3;
    assert!(
        ms < 50.0 || std::env::var_os("CADCRAFT3D_NO_BUDGETS").is_some(),
        "{ms} ms"
    );
}

/// The Files tab asks for these while the mouse walks over the rows (on a worker thread, but the
/// picture should follow the mouse).
#[test]
fn a_picture_for_the_files_tab_is_quick() {
    let path = std::path::Path::new("tests/data/scissors_blade.3dcadcraft");
    let t = std::time::Instant::now();
    cadcraft3d::document::format::read_preview(path).unwrap();
    let read = t.elapsed().as_secs_f64() * 1e3;
    let back = Project::load(path).unwrap();
    let mut part = parts(&back)[0].clone();
    part.regenerate().unwrap();
    let t = std::time::Instant::now();
    let png = thumbnail::part_png(&part, thumbnail::iso_view()).unwrap();
    let draw = t.elapsed().as_secs_f64() * 1e3;
    assert!(png.len() > 100);
    assert!(
        (read < 20.0 && draw < 150.0) || std::env::var_os("CADCRAFT3D_NO_BUDGETS").is_some(),
        "read {read} ms, draw {draw} ms"
    );
}

/// A ghost keeps the largest triangles (the big faces of a large assembly).
#[test]
fn a_ghost_keeps_the_largest_triangles() {
    let mesh = cadcraft3d::assembly::PickMesh::of(
        &PartDocument::with_box("Box1", [100.0, 10.0, 1.0]).unwrap(),
    );
    let keep = mesh.largest_triangles(4);
    assert_eq!(keep.len(), 4);
    let tris = mesh.triangles();
    let area = |k: usize| {
        let [a, b, c] = tris[k].1;
        (b - a).cross(&(c - a)).norm()
    };
    let least = keep
        .iter()
        .map(|k| area(*k as usize))
        .fold(f64::MAX, f64::min);
    let others = (0..tris.len())
        .filter(|k| !keep.contains(&(*k as u32)))
        .map(area)
        .fold(0.0, f64::max);
    assert!(least >= others, "{least} {others}");
}
