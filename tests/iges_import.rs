//! IGES through File → Export and File → Open / Import (spec 7.7 IGES row, spec 7.7a 3.15): a part
//! written as IGES and read back is the same solid; the import embeds STEP text (rule 8), and a
//! Reference Model update of a changed IGES file keeps the faces that stayed.

use std::path::Path;
use std::sync::Arc;

use cadcraft3d::kernel::Real;
use cadcraft3d::part::export::{ExportFormat, export_bytes};
use cadcraft3d::part::import::{ImportOp, SourceKind, source_text};
use cadcraft3d::part::{BlendFeature, BodyOp, BodyOpFeature, PartDocument};

fn rounded_box(height: Real) -> PartDocument {
    let mut part = PartDocument::with_box("Box1", [40.0, 30.0, height]).unwrap();
    let top = cadcraft3d::part::select::edges_of_face(
        &part.body,
        part.body.find_face("Box1:Top").unwrap(),
    );
    let refs: Vec<_> = top
        .into_iter()
        .filter_map(|e| cadcraft3d::part::blend::edge_ref(&part.body, e))
        .collect();
    part.add_blend(BlendFeature::fillet(refs, 3.0)).unwrap();
    part
}

fn volume(p: &PartDocument) -> Real {
    p.exact_bodies
        .0
        .iter()
        .map(|s| s.volume_at(1e-3).unwrap())
        .sum()
}

fn iges_of(p: &PartDocument) -> Vec<u8> {
    export_bytes(p, "Part1", ExportFormat::Iges).unwrap()
}

#[test]
fn iges_written_and_opened_again_is_the_same_solid() {
    let src = rounded_box(20.0);
    let bytes = iges_of(&src);
    let path = Path::new("bracket.IGS");
    assert_eq!(SourceKind::of(path), Some(SourceKind::Iges));
    let text = source_text(path, &bytes, 1.0).unwrap();
    assert!(text.starts_with("ISO-10303-21;"), "embedded as STEP");
    let mut p = PartDocument::default();
    p.add_body_op(BodyOpFeature::new(BodyOp::Import(ImportOp::new(
        "bracket.igs",
        text.as_str(),
    ))))
    .unwrap();
    assert_eq!(p.exact_bodies.0.len(), 1);
    assert!(!p.exact_bodies.0[0].is_open(), "a closed solid");
    let (want, got) = (volume(&src), volume(&p));
    assert!((got - want).abs() < 1e-3 * want, "{got} vs {want}");
    // The import's faces are named by their STEP entities, so a later feature can use them.
    assert!(p.body.faces().all(|(_, f)| {
        f.name
            .as_deref()
            .is_some_and(|n| n.starts_with("Import1:F"))
    }),);
}

#[test]
fn reference_model_update_of_a_changed_iges_file() {
    let a = source_text(Path::new("a.igs"), &iges_of(&rounded_box(20.0)), 1.0).unwrap();
    let b = source_text(Path::new("a.igs"), &iges_of(&rounded_box(25.0)), 1.0).unwrap();
    let op = ImportOp::new("a.igs", a.as_str());
    let mut p = PartDocument::default();
    let id = p
        .add_body_op(BodyOpFeature::new(BodyOp::Import(op.clone())))
        .unwrap();
    let (_, report) = op.updated(&id, Arc::from(b.as_str())).unwrap();
    // A taller box: the top, the bottom and the rounds keep their names at least.
    assert!(report.matched >= 2, "{report:?}");
}

#[test]
fn not_iges_is_an_error_not_a_panic() {
    assert!(source_text(Path::new("x.iges"), b"not an iges file", 1.0).is_err());
    assert!(source_text(Path::new("x.iges"), b"", 1.0).is_err());
}
