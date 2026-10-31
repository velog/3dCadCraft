//! Linked parameters (spec 3.10 §3.9) on a real furniture case: a cabinet's
//! master parameter part (Width, Height, Depth) drives its side panel and
//! shelf; the same values from a CSV sheet; a part derived with its
//! parameters; Break Link and a missing source.

use cadcraft3d::assembly::make;
use cadcraft3d::document::{DocRef, DocumentData, DocumentId, Project};
use cadcraft3d::kernel::Real;
use cadcraft3d::part::PartDocument;
use cadcraft3d::part::derive::DeriveLink;
use cadcraft3d::part::expr::Unit;
use cadcraft3d::part::params::ParamLink;

/// A part holding only user parameters (Inventor's master parameter file).
fn master(width: Real, height: Real) -> PartDocument {
    let mut p = PartDocument::default();
    for (n, v) in [("Width", width), ("Height", height), ("Depth", 350.0)] {
        let u = p.add_user_parameter(Unit::Mm);
        p.rename_parameter(&u, n).unwrap();
        p.set_parameter_expr(n, &format!("{v} mm")).unwrap();
    }
    p
}

/// A box panel whose size parameters follow `x` / `y` / `z` (names of this
/// part's parameters or numbers).
fn panel(link: &[(&str, ParamLink, Real)], x: &str, y: &str, z: &str) -> PartDocument {
    let mut p = PartDocument::with_box("Box1", [10.0, 10.0, 10.0]).unwrap();
    for (n, l, v) in link {
        p.add_linked_parameter(n, *v, Unit::Mm, l.clone()).unwrap();
    }
    let sizes: Vec<String> = p
        .parameter_table()
        .into_iter()
        .filter(|r| r.consumed_by.contains("Box1"))
        .map(|r| r.name)
        .collect();
    assert_eq!(sizes.len(), 3, "{sizes:?}");
    for (name, e) in sizes.iter().zip([x, y, z]) {
        p.set_parameter_expr(name, e).unwrap();
    }
    p.regenerate().unwrap();
    p
}

fn size(p: &PartDocument) -> [Real; 3] {
    let (lo, hi) = p.body.bounding_box().unwrap();
    [hi.x - lo.x, hi.y - lo.y, hi.z - lo.z]
}

fn part(p: &Project, d: DocumentId) -> &PartDocument {
    match p.document(d).map(|x| &x.data) {
        Some(DocumentData::Part(q)) => q,
        _ => panic!("no part"),
    }
}

fn part_mut(p: &mut Project, d: DocumentId) -> &mut PartDocument {
    match p.document_mut(d).map(|x| &mut x.data) {
        Some(DocumentData::Part(q)) => q,
        _ => panic!("no part"),
    }
}

#[test]
fn a_cabinets_panels_follow_its_master_parameters() {
    let mut p = Project::new("Cabinet");
    let m = p.add_document(
        "Cabinet parameters",
        DocumentData::Part(master(600.0, 900.0)),
    );
    let from = |n: &str| ParamLink::Document {
        source: DocRef::local(m),
        name: n.into(),
    };
    // The side: 18 thick, Depth deep, Height high.
    let side = panel(
        &[
            ("Height", from("Height"), 900.0),
            ("Depth", from("Depth"), 350.0),
        ],
        "18 mm",
        "Depth",
        "Height",
    );
    // A shelf between the sides: Width − 2·18 long.
    let shelf = panel(
        &[
            ("Width", from("Width"), 600.0),
            ("Depth", from("Depth"), 350.0),
        ],
        "Width - 36 mm",
        "Depth - 20 mm",
        "18 mm",
    );
    let s = p.add_document("Side", DocumentData::Part(side));
    let sh = p.add_document("Shelf", DocumentData::Part(shelf));
    assert!(make::update_all(&mut p).0.is_empty(), "nothing changed yet");
    // The cabinet becomes 800 wide and 1200 high.
    let mp = part_mut(&mut p, m);
    mp.set_parameter_expr("Width", "800 mm").unwrap();
    mp.set_parameter_expr("Height", "1200 mm").unwrap();
    let (changed, errors) = make::update_all(&mut p);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(changed.len(), 2);
    let [_, _, h] = size(part(&p, s));
    assert!((h - 1200.0).abs() < 1e-6, "{h}");
    let [w, d, _] = size(part(&p, sh));
    assert!(
        (w - 764.0).abs() < 1e-6 && (d - 330.0).abs() < 1e-6,
        "{w} {d}"
    );
    // Linked values are plain values in the file: the side builds alone.
    let mut alone = part(&p, s).clone();
    alone.regenerate().unwrap();
    assert!((size(&alone)[2] - 1200.0).abs() < 1e-6);
}

#[test]
fn a_csv_sheet_drives_the_parameters() {
    let dir = std::env::temp_dir().join(format!("cadcraft-csv-{}", DocumentId::new_random()));
    std::fs::create_dir_all(&dir).unwrap();
    let csv = dir.join("cabinet.csv");
    std::fs::write(
        &csv,
        "Name,Value,Unit\nHeight,900,mm\nShelves,4,ul\n# a comment\nDepth,350 mm\n",
    )
    .unwrap();
    let rows = cadcraft3d::part::params::csv_parameters(&std::fs::read_to_string(&csv).unwrap());
    assert_eq!(rows.len(), 3, "{rows:?}");
    let path = csv.display().to_string();
    let link = ParamLink::Csv {
        path: path.clone(),
        name: "Height".into(),
    };
    let mut p = Project::new("Sheet");
    let s = p.add_document(
        "Side",
        DocumentData::Part(panel(
            &[("Height", link, 900.0)],
            "18 mm",
            "350 mm",
            "Height",
        )),
    );
    std::fs::write(&csv, "Height,1000,mm\nShelves,5,ul\nDepth,350 mm\n").unwrap();
    let (changed, errors) = make::update_all(&mut p);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(changed, vec![s]);
    assert!((size(part(&p, s))[2] - 1000.0).abs() < 1e-6);
}

#[test]
fn a_part_derived_with_its_parameters_links_them() {
    let mut src = master(600.0, 900.0);
    let _ = src.regenerate();
    let mut box_src = PartDocument::with_box("Box1", [600.0, 350.0, 18.0]).unwrap();
    box_src.user_params = src.user_params.clone();
    let mut p = Project::new("Derived");
    let m = p.add_document("Top panel", DocumentData::Part(box_src.clone()));
    let mut l = DeriveLink::new(DocRef::local(m), "Top panel");
    l.parameters = true;
    let mut d = PartDocument::default();
    d.add_derive(&box_src, l).unwrap();
    for n in ["Width", "Height", "Depth"] {
        assert!(
            d.user_params
                .iter()
                .any(|u| u.name == n && u.link.is_some()),
            "{n}"
        );
    }
    let did = p.add_document("Derived", DocumentData::Part(d));
    part_mut(&mut p, m)
        .set_parameter_expr("Width", "650 mm")
        .unwrap();
    make::update_all(&mut p);
    let w = part(&p, did).parameter("Width").unwrap().value;
    assert!((w - 650.0).abs() < 1e-9, "{w}");
}

#[test]
fn break_link_keeps_the_value_and_a_missing_source_is_reported() {
    let mut p = Project::new("Cabinet");
    let m = p.add_document("Master", DocumentData::Part(master(600.0, 900.0)));
    let link = ParamLink::Document {
        source: DocRef::local(m),
        name: "Height".into(),
    };
    let mut side = panel(
        &[("Height", link.clone(), 900.0)],
        "18 mm",
        "350 mm",
        "Height",
    );
    // A second link of the same name becomes Height_2.
    let n = side
        .add_linked_parameter("Height", 900.0, Unit::Mm, link)
        .unwrap();
    assert_eq!(n, "Height_2");
    let s = p.add_document("Side", DocumentData::Part(side));
    // The master is deleted: the last value stays, the error is reported.
    p.remove_document(m).unwrap();
    let (changed, errors) = make::update_all(&mut p);
    assert!(changed.is_empty());
    assert!(!errors.is_empty());
    assert!((size(part(&p, s))[2] - 900.0).abs() < 1e-6);
    // Break Link: a plain parameter now.
    let sp = part_mut(&mut p, s);
    assert!(sp.break_param_link("Height"));
    assert!(
        sp.user_params
            .iter()
            .find(|u| u.name == "Height")
            .unwrap()
            .link
            .is_none()
    );
}
