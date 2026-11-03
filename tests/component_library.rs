//! The library's catalog and publishing (spec 6.3): families listed by
//! category, members written once with stable identities, a family of the
//! user's own going from a part's table to a linked member.

use std::path::PathBuf;

use cadcraft3d::components::catalog::{Catalog, publish};
use cadcraft3d::document::iprops::IProperties;
use cadcraft3d::document::{DocumentData, Project};
use cadcraft3d::part::PartDocument;
use cadcraft3d::part::expr::Unit;
use cadcraft3d::part::family::{Column, FamilyTable, Target};
use cadcraft3d::part::physical::physical_properties;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("cc-library-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn block() -> PartDocument {
    let mut part = PartDocument::with_box("Box1", [10.0, 20.0, 30.0]).unwrap();
    part.rename_parameter("d0", "length").unwrap();
    let mut len = Column::new("L", Unit::Mm, Target::Parameter("length".into()));
    len.key = 1;
    let mut pn = Column::new("PN", Unit::Text, Target::Property("part_number".into()));
    pn.formula = Some("\"Block \" + L".into());
    part.family = Some(FamilyTable {
        id: "block-family".into(),
        columns: vec![len, pn],
        rows: vec![
            vec!["10".into(), String::new()],
            vec!["25".into(), String::new()],
        ],
        default_row: 0,
    });
    part
}

fn volume_in(path: &std::path::Path) -> f64 {
    let p = Project::load(path).unwrap();
    let DocumentData::Part(part) = &p.documents()[0].data else {
        panic!("not a part");
    };
    let mut part = part.clone();
    part.regenerate().unwrap();
    physical_properties(&part, Default::default())
        .unwrap()
        .total
        .volume
}

#[test]
fn samples_are_listed_by_category_and_found() {
    let root = tmp("list");
    let c = Catalog::load(&root);
    assert!(
        c.categories()
            .contains(&"Fasteners/Bolts/Hex Head".to_owned())
    );
    assert!(
        c.categories().contains(&"Fasteners".to_owned()),
        "parents too"
    );
    let bolts = c.in_category("Fasteners/Bolts");
    assert!(bolts.iter().any(|&f| c.families[f].name == "ISO 4017"));
    assert!(bolts.iter().any(|&f| c.families[f].name == "ISO 4762"));
    assert!(!bolts.iter().any(|&f| c.families[f].name == "ISO 4032"));
    assert_eq!(c.in_category("").len(), c.families.len());
    let hits = c.search("iso 4017 m8 x 30");
    assert_eq!(hits.len(), 1);
    let (f, m) = hits[0];
    assert_eq!(c.families[f].members[m].size, "M8 x 30");
    assert!(c.search("no such part").is_empty());
}

#[test]
fn a_sample_member_is_written_once_when_it_is_asked_for() {
    let root = tmp("once");
    let c = Catalog::load(&root);
    let (f, m) = c.search("iso 7089 8")[0];
    let path = c.member_file(f, m).unwrap();
    assert!(path.exists());
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(c.member_file(f, m).unwrap(), path);
    assert_eq!(std::fs::read(&path).unwrap(), bytes, "not written again");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_published_family_lists_its_rows_and_makes_members() {
    let root = tmp("publish");
    publish(
        &root,
        "Own Parts/Blocks",
        "Block",
        &block(),
        &IProperties::default(),
    )
    .unwrap();
    let c = Catalog::load(&root);
    assert!(c.warnings.is_empty(), "{:?}", c.warnings);
    let f = c
        .families
        .iter()
        .position(|x| x.name == "Block")
        .expect("the family is listed");
    assert_eq!(c.families[f].category, "Own Parts/Blocks");
    let sizes: Vec<&str> = c.families[f]
        .members
        .iter()
        .map(|m| m.size.as_str())
        .collect();
    assert_eq!(sizes, ["10", "25"]);

    let path = c.member_file(f, 1).unwrap();
    assert!((volume_in(&path) - 25.0 * 20.0 * 30.0).abs() < 1e-6);
    let p = Project::load(&path).unwrap();
    assert_eq!(p.documents()[0].properties.get("part_number"), "Block 25");
    let id = p.id;
    let doc = p.documents()[0].id;
    let DocumentData::Part(member) = &p.documents()[0].data else {
        panic!("not a part");
    };
    assert!(member.family.is_none(), "a member holds no table");

    // Deleted and made again: the same identity, so links still resolve.
    std::fs::remove_file(&path).unwrap();
    let again = Project::load(&c.member_file(f, 1).unwrap()).unwrap();
    assert_eq!(again.id, id);
    assert_eq!(again.documents()[0].id, doc);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_bad_table_or_no_table_is_refused_and_writes_nothing() {
    let root = tmp("refuse");
    let props = IProperties::default();
    let plain = PartDocument::with_box("Box1", [1.0, 1.0, 1.0]).unwrap();
    assert!(publish(&root, "A", "B", &plain, &props).is_err());
    let mut bad = block();
    if let Some(t) = bad.family.as_mut() {
        t.rows[1][0] = "10".into();
    }
    let e = publish(&root, "A", "B", &bad, &props).unwrap_err();
    assert!(e.contains("the same keys"), "{e}");
    assert!(publish(&root, "A", "  ", &block(), &props).is_err());
    assert!(!root.exists(), "nothing written");
}

#[test]
fn a_family_name_cannot_leave_the_library_folder() {
    let root = tmp("names");
    let path = publish(
        &root,
        "A/../../B",
        "x/../y:z",
        &block(),
        &IProperties::default(),
    )
    .unwrap();
    let path = path.canonicalize().unwrap();
    let root = root.canonicalize().unwrap();
    assert!(path.starts_with(&root), "{}", path.display());
    let _ = std::fs::remove_dir_all(&root);
}

/// Every table file in `data/components/families` is built in.
#[test]
fn every_table_file_is_built_in() {
    let mut files: Vec<String> = std::fs::read_dir(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/data/components/families"
    ))
    .unwrap()
    .map(|e| {
        e.unwrap()
            .path()
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .into_owned()
    })
    .collect();
    files.sort();
    let mut ids: Vec<String> = cadcraft3d::components::samples::table_ids()
        .into_iter()
        .map(str::to_owned)
        .collect();
    ids.sort();
    assert_eq!(ids, files);
}

/// The families whose type has a builder are all listed, with every length.
#[test]
fn the_tables_reach_the_catalog() {
    let c = Catalog::load(&tmp("tables"));
    let family = |name: &str| c.families.iter().find(|f| f.name == name);
    let bolts = family("ISO 4017").expect("ISO 4017");
    assert_eq!(bolts.category, "Fasteners/Bolts/Hex Head");
    assert!(bolts.members.iter().any(|m| m.size == "M36 x 200"));
    assert_eq!(family("ISO 7089").unwrap().members.len(), 13);
    assert_eq!(family("Series 62").unwrap().members.len(), 11);
    assert!(
        family("IPE beam (EN 10365)").is_some(),
        "the frame sections too"
    );
    assert!(family("ISO 10642").is_some());
    assert!(
        c.families
            .iter()
            .all(|f| !f.category.starts_with("Pipe Parts")),
        "no builder yet"
    );
    let total: usize = c.families.iter().map(|f| f.members.len()).sum();
    println!("{} families, {total} members", c.families.len());
    assert!(total > 500, "{total}");
}

#[test]
fn families_carry_their_standard_and_inch_sizes_read_as_inches() {
    let c = Catalog::load(&tmp("standards"));
    assert_eq!(c.standards()[..4], ["ISO", "DIN", "ANSI", "EN"]);
    let family = |name: &str| {
        c.families
            .iter()
            .position(|f| f.name == name)
            .unwrap_or_else(|| panic!("no {name}"))
    };
    assert_eq!(c.families[family("ISO 4017")].standard, "ISO");
    assert_eq!(c.families[family("DIN 933")].standard, "DIN");
    assert_eq!(c.families[family("Equal angle (AISC)")].standard, "ANSI");
    assert_eq!(c.families[family("IPE beam (EN 10365)")].standard, "EN");

    let ansi = family("ASME B18.2.1");
    assert_eq!(c.families[ansi].standard, "ANSI");
    assert!(
        c.families[ansi]
            .members
            .iter()
            .any(|m| m.size == "1/4-20 x 1-1/2"),
        "{:?}",
        c.families[ansi]
            .members
            .iter()
            .map(|m| &m.size)
            .take(4)
            .collect::<Vec<_>>()
    );
    assert!(c.in_standard(ansi, Some("ANSI")) && c.in_standard(ansi, None));
    assert!(!c.in_standard(ansi, Some("ISO")));

    // A filter leaves only the categories holding that standard.
    let en = c.categories_of(Some("EN"));
    assert!(
        en.iter().all(|x| x.starts_with("Structural Shapes")),
        "{en:?}"
    );
    assert!(
        c.categories_of(Some("ANSI"))
            .contains(&"Fasteners/Bolts/Hex Head".to_owned())
    );
}

#[test]
fn an_inch_bolt_builds_with_its_unc_thread_and_a_file_name_without_slashes() {
    let root = tmp("inch");
    let c = Catalog::load(&root);
    let (f, m) = c.search("asme b18.2.1 1/4-20 x 1")[0];
    let path = c.member_file(f, m).unwrap();
    assert!(path.exists());
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    assert!(!name.contains('/') && name.contains("1_4-20"), "{name}");
    let p = Project::load(&path).unwrap();
    assert!(
        p.documents()[0]
            .properties
            .get("part_number")
            .contains("1/4-20")
    );
    let DocumentData::Part(part) = &p.documents()[0].data else {
        panic!("not a part");
    };
    let mut part = part.clone();
    part.regenerate().unwrap();
    assert_eq!(part.cosmetic_threads.len(), 1);
    assert!((part.cosmetic_threads[0].pitch - 1.27).abs() < 1e-3);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn families_have_a_picture_and_it_comes_from_the_cache_the_second_time() {
    let root = tmp("thumbnail");
    let c = Catalog::load(&root);
    let f = c
        .families
        .iter()
        .position(|f| f.name == "ISO 4032")
        .unwrap_or_else(|| panic!("no ISO 4032"));
    let side = cadcraft3d::components::catalog::THUMBNAIL as usize;
    let rgba = c.thumbnail(f).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(rgba.len(), side * side * 4);
    assert!(
        rgba.chunks(4).any(|p| p[3] > 0),
        "the picture must show something"
    );
    let cached = std::fs::read_dir(root.join(".thumbnails"))
        .unwrap_or_else(|e| panic!("{e}"))
        .count();
    assert_eq!(cached, 1);
    assert_eq!(c.thumbnail(f).unwrap_or_else(|e| panic!("{e}")), rgba);
    assert_eq!(
        std::fs::read_dir(root.join(".thumbnails"))
            .unwrap_or_else(|e| panic!("{e}"))
            .count(),
        1,
        "the second call reads the same picture"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn sources_that_disagree_give_a_member_each() {
    let c = Catalog::load(&tmp("both_values"));
    let find = |family: &str, size: &str| {
        c.families
            .iter()
            .find(|f| f.name == family)
            .is_some_and(|f| f.members.iter().any(|m| m.size == size))
    };
    assert!(find("ISO 4032", "M6"));
    assert!(find("ISO 4032", "M6 (m 5)"));
    assert!(find("DIN 1587", "M10 (s 16)"));
    assert!(find("Series R", "R6 (B 5.558)"));
}

#[test]
fn a_family_tells_what_its_table_was_compared_with() {
    let c = Catalog::load(&tmp("checked"));
    let of = |name: &str| {
        c.families
            .iter()
            .find(|f| f.name == name)
            .map(|f| f.checked.clone())
            .unwrap_or_else(|| panic!("no {name}"))
    };
    assert_eq!(of("Series 68"), "Koyo");
    assert_eq!(of("Metric O-Ring"), "");
}

/// A published family with a connection: droppable, each member at the diameter
/// its column gives, and Change Size finds a placed member's row.
#[test]
fn a_published_family_goes_by_its_connection() {
    use cadcraft3d::components::fit::{Fit, Target, choose};
    use cadcraft3d::part::blend::EdgeRef;
    use cadcraft3d::part::connection::{Connection, ConnectionRole};
    let root = tmp("published_fit");
    let mut part = block();
    part.connections.push(Connection {
        name: "Connection1".into(),
        role: ConnectionRole::Into,
        edge: Some(EdgeRef {
            faces: ["a".into(), "b".into()],
            hint: [0.0; 3],
        }),
        diameter: 1.0,
        diameter_column: Some("L".into()),
    });
    publish(&root, "Own", "Pegs", &part, &IProperties::default()).unwrap();
    let c = Catalog::load(&root);
    let f = c.families.iter().position(|x| x.name == "Pegs").unwrap();
    let fits = c.fits(f);
    assert!(matches!(fits[0], Fit::Into { d, .. } if (d - 10.0).abs() < 1e-9));
    assert!(matches!(fits[1], Fit::Into { d, .. } if (d - 25.0).abs() < 1e-9));
    let hole = |diameter| Target {
        hole: true,
        diameter,
        thread: None,
        depth: 5.0,
        blind: false,
    };
    assert_eq!(choose(&fits, &hole(26.0)), Some(1));
    assert_eq!(choose(&fits, &hole(11.0)), Some(0));

    // Without a connection it stays by hand.
    publish(&root, "Own", "Plain", &block(), &IProperties::default()).unwrap();
    let c = Catalog::load(&root);
    let plain = c.families.iter().position(|x| x.name == "Plain").unwrap();
    assert!(c.fits(plain).iter().all(|x| !x.droppable()));

    let f = c.families.iter().position(|x| x.name == "Pegs").unwrap();
    let path = c.member_file(f, 1).unwrap();
    assert_eq!(c.locate(&path), Some((f, 1)));
    let _ = std::fs::remove_dir_all(&root);
}
