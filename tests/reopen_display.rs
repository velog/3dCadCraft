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
