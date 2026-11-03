//! The sample Component Library (spec 6.3, stage CC-S): every sample builds
//! exactly, weighs what its shape says, survives a save and load, and writing
//! the library never replaces a file assemblies may link.

use std::f64::consts::PI;
use std::path::PathBuf;
use std::time::Instant;

use cadcraft3d::components::catalog::Catalog;
use cadcraft3d::components::samples::{Detail, Kind, Sample, all};
use cadcraft3d::document::{AssemblyDocument, DocRef, DocumentData, Project, Resolver};
use cadcraft3d::kernel::Transform3;
use cadcraft3d::part::PartDocument;
use cadcraft3d::part::physical::physical_properties;

/// The first and last member of each family, and every 9th: all kinds and the
/// smallest and largest sizes without building hundreds of parts per test.
fn spread() -> Vec<Sample> {
    let every = all();
    let mut out: Vec<Sample> = Vec::new();
    for (k, s) in every.iter().enumerate() {
        let first = k == 0 || every[k - 1].family != s.family;
        let last = k + 1 == every.len() || every[k + 1].family != s.family;
        if first || last || k % 9 == 0 {
            out.push(s.clone());
        }
    }
    out
}

fn volume(p: &PartDocument) -> f64 {
    physical_properties(p, Default::default())
        .unwrap()
        .total
        .volume
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("cc-samples-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

#[test]
fn every_sample_builds_exactly_and_has_its_properties() {
    let mut slowest = (0.0, String::new());
    let all_start = Instant::now();
    for s in spread() {
        let t = Instant::now();
        let p = s
            .part()
            .unwrap_or_else(|e| panic!("{}: {e}", s.designation()));
        let ms = t.elapsed().as_secs_f64() * 1e3;
        println!("{:<44} {:>6.0} ms", s.designation(), ms);
        if ms > slowest.0 {
            slowest = (ms, s.designation());
        }
        assert!(
            p.exact,
            "{} fell back to the faceted kernel",
            s.designation()
        );
        assert!(
            p.feature_health.is_empty(),
            "{}: {:?}",
            s.designation(),
            p.feature_health
        );
        assert!(volume(&p) > 0.0, "{}", s.designation());
        assert!(p.material.is_some(), "{} has no material", s.designation());
        let props = s.properties();
        assert_eq!(props.get("part_number"), s.designation());
        assert!(!props.get("description").is_empty());
    }
    println!("slowest sample: {} {:.0} ms", slowest.1, slowest.0);
    let total = all_start.elapsed().as_secs_f64();
    println!("all samples: {:.0} ms", total * 1e3);
}

#[test]
fn volumes_match_the_shapes() {
    for s in spread() {
        let p = s.part().unwrap();
        let v = volume(&p);
        let expected = match s.kind {
            Kind::Washer(d1, d2, h) => PI / 4.0 * (d2 * d2 - d1 * d1) * h,
            Kind::Key(b, h, l) => (b * (l - b) + PI * b * b / 4.0) * h,
            Kind::Circlip(d, g, t) => {
                let (r0, r1) = (g / 2.0, g / 2.0 + 0.12 * d);
                PI * (r1 * r1 - r0 * r0) * t * (330.0 / 360.0)
            }
            Kind::Pin(d, l) => {
                let (r, c) = (d / 2.0, d / 8.0);
                // A cylinder and two cone frusta.
                let end = PI * c / 3.0 * (r * r + r * (r - c) + (r - c) * (r - c));
                PI * r * r * (l - 2.0 * c) + 2.0 * end
            }
            Kind::Rivet(d, l, dk, k) => {
                let a = dk / 2.0;
                PI * d * d / 4.0 * l + PI * k / 6.0 * (3.0 * a * a + k * k)
            }
            _ => continue,
        };
        // The volume comes from the display triangles, inscribed ~0.01 mm
        // inside a round: that weighs more the smaller the part.
        let size = expected.cbrt();
        assert!(
            (v - expected).abs() < expected * (5e-3 + 0.05 / size),
            "{}: {v} vs {expected}",
            s.designation()
        );
    }
}

#[test]
fn fasteners_have_a_cosmetic_thread_and_a_hexagon_across_flats() {
    for s in spread() {
        let p = s.part().unwrap();
        match s.kind {
            Kind::HexBolt(_, l, a, k) => {
                assert_eq!(p.cosmetic_threads.len(), 1, "{}", s.designation());
                let b = p.body.bounding_box().unwrap();
                assert!((b.0.z + l).abs() < 1e-6 && (b.1.z - k).abs() < 1e-6);
                // Flats at ±y.
                assert!((b.1.y - a / 2.0).abs() < 1e-6, "{}: {b:?}", s.designation());
            }
            Kind::PartThreadBolt(d, l, ..) => {
                assert_eq!(p.cosmetic_threads.len(), 1, "{}", s.designation());
                let b = 2.0 * d + if l > 125.0 { 12.0 } else { 6.0 };
                let t = &p.cosmetic_threads[0];
                assert!(
                    (t.length - b.min(l)).abs() < l * 0.2,
                    "{}: {t:?}",
                    s.designation()
                );
                assert!(t.pitch > 0.0);
            }
            Kind::HexNut(d, a, m) => {
                assert_eq!(p.cosmetic_threads.len(), 1, "{}", s.designation());
                // Hexagon less bore (the minor diameter is about 0.85 d, 0.79 d for a
                // coarse small inch thread).
                let d = d * 0.85;
                let full = (3f64.sqrt() / 2.0 * a * a - PI * d * d / 4.0) * m;
                let v = volume(&p);
                assert!(
                    (v - full).abs() < full * 0.06,
                    "{}: {v} vs {full}",
                    s.designation()
                );
            }
            Kind::SocketScrew(..) => {
                assert_eq!(p.cosmetic_threads.len(), 1, "{}", s.designation());
            }
            _ => {}
        }
    }
}

#[test]
fn a_member_is_saved_as_a_file_and_never_replaced() {
    let dir = tmp("write");
    let catalog = Catalog::load(&dir);
    for &(f, m) in &[
        catalog.search("iso 4017 m8 x 30")[0],
        catalog.search("iso 4032 m8")[0],
        catalog.search("din 471 20")[0],
        catalog.search("series 62 6204")[0],
        catalog.search("ipe 200")[0],
    ] {
        let path = catalog.member_file(f, m).unwrap();
        let before = std::fs::read(&path).unwrap();
        assert_eq!(catalog.member_file(f, m).unwrap(), path);
        assert_eq!(std::fs::read(&path).unwrap(), before, "written again");
        let project = Project::load(&path).unwrap();
        let doc = project.documents().first().unwrap();
        let fam = &catalog.families[f];
        let size = &fam.members[m].size;
        let designation = doc.properties.get("part_number").to_owned();
        assert!(designation.ends_with(size.as_str()), "{designation}");
        let DocumentData::Part(loaded) = &doc.data else {
            panic!("{designation} is not a part");
        };
        let mut loaded = loaded.clone();
        loaded.regenerate().unwrap();
        assert!(volume(&loaded) > 0.0, "{designation}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// How a library component is used and saved: linked by a relative path, many
/// occurrences of one file, and the assembly reopens with the file's name and
/// properties.
#[test]
fn an_assembly_links_library_files_and_reopens() {
    let root = tmp("assembly");
    let lib = root.join("library");
    let catalog = Catalog::load(&lib);
    let (f, m) = catalog.search("iso 4017 m8 x 30")[0];
    let work = root.join("work");
    std::fs::create_dir_all(&work).unwrap();
    let path = work.join("a.3dcadcraft");

    let file = catalog.member_file(f, m).unwrap();
    let mut project = Project::new("a");
    let asm = project.add_document("a", DocumentData::Assembly(AssemblyDocument::default()));
    project.save(&path).unwrap();

    let mut resolver = Resolver::new();
    let id = cadcraft3d::document::storage::link_file(&mut project, &path, &mut resolver, &file)
        .unwrap();
    let linked = resolver.load(&file).unwrap();
    let part_doc = linked.documents().first().unwrap().id;
    let r = DocRef {
        variant: None,
        library: Some(id),
        document: part_doc,
    };
    let Some(DocumentData::Assembly(a)) = project.document_mut(asm).map(|d| &mut d.data) else {
        panic!("no assembly");
    };
    for k in 0..3 {
        a.place(r, &format!("Bolt:{k}"), Transform3::identity(), false);
    }
    project.save(&path).unwrap();

    let loaded = Project::load(&path).unwrap();
    let link = &loaded.libraries()[0];
    assert!(!link.path.contains(":"), "relative path: {}", link.path);
    assert!(
        link.path.ends_with("ISO 4017 M8 x 30.ccpart"),
        "{}",
        link.path
    );
    let DocumentData::Assembly(a) = &loaded.document(asm).unwrap().data else {
        panic!("no assembly");
    };
    assert_eq!(a.occurrences.len(), 3);
    let target = work.join(&link.path);
    let again = Project::load(&target).unwrap();
    assert_eq!(
        again.documents()[0].properties.get("part_number"),
        "ISO 4017 M8 x 30"
    );

    // The library deleted and written again: the old link still resolves.
    std::fs::remove_dir_all(&lib).unwrap();
    catalog.member_file(f, m).unwrap();
    let mut fresh = Resolver::new();
    let found = fresh.load_library(&path, &loaded.libraries()[0]).unwrap();
    assert!(
        found.1.document(part_doc).is_some(),
        "the part keeps its id"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// Every member of every table builds (several minutes: `--ignored`, in
/// `cargo xtask heavy`).
#[test]
#[ignore = "builds hundreds of parts"]
fn every_member_of_the_tables_builds() {
    let mut failed = Vec::new();
    let every = all();
    for s in &every {
        match s.part() {
            Ok(p) if p.exact && p.feature_health.is_empty() => {}
            Ok(p) => failed.push(format!("{}: {:?}", s.designation(), p.feature_health)),
            Err(e) => failed.push(format!("{}: {e}", s.designation())),
        }
    }
    println!("{} members, {} failed", every.len(), failed.len());
    assert!(failed.is_empty(), "{failed:#?}");
}

/// One member per family with its time, printed before it starts: shows what a
/// slow or stuck kernel call is. `--ignored --nocapture`.
#[test]
#[ignore = "diagnostic"]
fn first_of_each_family_with_times() {
    use std::io::Write;
    let mut seen = std::collections::BTreeSet::new();
    for s in all() {
        if !seen.insert(s.family.clone()) {
            continue;
        }
        print!("{:<44}", s.designation());
        let _ = std::io::stdout().flush();
        let t = Instant::now();
        let r = s.part();
        let ms = t.elapsed().as_secs_f64() * 1e3;
        match r {
            Ok(p) if p.exact && p.feature_health.is_empty() => println!("{ms:>8.0} ms"),
            Ok(p) => println!("{ms:>8.0} ms SICK {:?} exact={}", p.feature_health, p.exact),
            Err(e) => println!("{ms:>8.0} ms ERROR {e}"),
        }
    }
}

fn find(designation: &str) -> Sample {
    all()
        .into_iter()
        .find(|s| s.designation() == designation)
        .unwrap_or_else(|| panic!("no {designation}"))
}

#[test]
fn three_levels_of_one_bolt() {
    let s = find("ISO 4017 M8 x 30");
    let simple = s.part_at(Detail::Simple).unwrap();
    let standard = s.part_at(Detail::Standard).unwrap();
    let full = s.part_at(Detail::Full).unwrap();
    assert!(
        simple.cosmetic_threads.is_empty(),
        "the envelope has no thread"
    );
    assert_eq!(standard.cosmetic_threads.len(), 1);
    assert_eq!(full.cosmetic_threads.len(), 1);
    assert!(simple.exact && standard.exact && full.exact);
    let (a, b, c) = (volume(&simple), volume(&standard), volume(&full));
    // Round head past the corners > plain hexagon > chamfered hexagon.
    assert!(a > b && b > c, "{a} {b} {c}");
    assert!((b - c) / b < 0.02, "the chamfers take little: {b} {c}");
}

#[test]
fn a_full_bearing_has_rings_and_rolling_elements() {
    let s = find("6204");
    let full = s.part_at(Detail::Full).unwrap();
    assert_eq!(full.body.solids().count(), 3);
    let standard = s.part_at(Detail::Standard).unwrap();
    assert_eq!(standard.body.solids().count(), 1);
    assert!(volume(&full) < volume(&standard));
}

#[test]
fn levels_are_separate_files_of_one_component() {
    let dir = tmp("levels");
    let c = Catalog::load(&dir);
    let (f, m) = c.search("iso 4032 m8")[0];
    assert_eq!(c.levels(f, m), Detail::ALL);
    let paths: Vec<_> = Detail::ALL
        .iter()
        .map(|d| c.member_file_at(f, m, *d).unwrap())
        .collect();
    assert!(paths[0] != paths[1] && paths[1] != paths[2], "{paths:?}");
    let ids: Vec<_> = paths.iter().map(|p| Project::load(p).unwrap().id).collect();
    assert!(ids[0] != ids[1] && ids[1] != ids[2]);
    for p in &paths {
        let project = Project::load(p).unwrap();
        assert_eq!(
            project.documents()[0].properties.get("part_number"),
            "ISO 4032 M8"
        );
    }
    // A pin has one shape: every level is its one file.
    let (f, m) = c.search("iso 2338 6 x 30")[0];
    assert_eq!(c.levels(f, m), [Detail::Standard]);
    assert_eq!(
        c.member_file_at(f, m, Detail::Full).unwrap(),
        c.member_file(f, m).unwrap()
    );
    let _ = std::fs::remove_dir_all(&dir);
}
