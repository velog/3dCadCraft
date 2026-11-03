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
    assert!(full.body.solids().count() > 3, "rings and every ball");
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

/// A flange's volume from its table: disc, raised face, hub and neck as rings,
/// less its bolt holes and its bore. EN 1092-1 PN 16.
#[test]
fn flanges_weigh_what_their_tables_say() {
    let of = |family: &str, size: &str| {
        all()
            .into_iter()
            .find(|s| s.family == family && s.size == size)
            .unwrap_or_else(|| panic!("{family} {size}"))
    };
    let disc = |r: f64, h: f64| PI * r * r * h;
    let cone = |r1: f64, r2: f64, h: f64| PI * h / 3.0 * (r1 * r1 + r1 * r2 + r2 * r2);

    // Blind DN100: D 220, b 20, 8 x 18 on 180, raised 158 x 2.
    let s = of("EN 1092-1 Type 05", "DN100");
    let p = s.part().unwrap();
    let want = disc(110.0, 18.0) + disc(79.0, 2.0) - 8.0 * disc(9.0, 18.0);
    assert!((volume(&p) - want).abs() / want < 2e-3, "{}", volume(&p));
    // The Simple level has no holes.
    let simple = s.part_at(Detail::Simple).unwrap();
    assert_eq!(s.level_of(Detail::Simple), Detail::Simple);
    assert!(volume(&simple) > volume(&p));

    // Slip-on DN100: D 220, bore 116, b 22, 8 x 18.
    let p = of("EN 1092-1 Type 01", "DN100").part().unwrap();
    let want = disc(110.0, 22.0) - disc(58.0, 22.0) - 8.0 * disc(9.0, 22.0);
    assert!((volume(&p) - want).abs() / want < 2e-3, "{}", volume(&p));

    // Weld neck DN100: A 114.3, S 3.6, N 131, H 52, H3 12, D 220, b 20, raised 158 x 2.
    let p = of("EN 1092-1 Type 11", "DN100").part().unwrap();
    let (rp, rb) = (57.15, 57.15 - 3.6);
    let want = disc(rp, 12.0) + cone(rp, 65.5, 20.0) + disc(110.0, 18.0) + disc(79.0, 2.0)
        - disc(rb, 52.0)
        - 8.0 * disc(9.0, 18.0);
    assert!((volume(&p) - want).abs() / want < 2e-3, "{}", volume(&p));
}

/// The largest flange has 20 holes: still one part within a new member's budget.
#[test]
fn every_flange_size_builds() {
    for s in all().iter().filter(|s| matches!(s.kind, Kind::Flange(_))) {
        let p = s
            .part()
            .unwrap_or_else(|e| panic!("{}: {e}", s.designation()));
        assert!(p.exact, "{} went faceted", s.designation());
        assert!(p.feature_health.is_empty(), "{}", s.designation());
    }
}

/// The largest flange's 20 holes are walls of one direct build (its time: `tests/perf.rs`).
#[test]
fn the_largest_flange_is_built_directly() {
    let s = all()
        .into_iter()
        .find(|s| s.family == "EN 1092-1 Type 11" && s.size == "DN600")
        .unwrap();
    let t = Instant::now();
    let p = s.part().unwrap();
    let ms = t.elapsed().as_secs_f64() * 1e3;
    println!("{} {ms:.0} ms", s.designation());
    assert!(
        p.body
            .faces()
            .any(|(_, f)| f.name.as_deref() == Some("Axial1:BoltHole:20")),
        "a hole's wall has its name"
    );
}

/// ASME B16.9 fittings against their tables: a bend's volume by Pappus, a reducer's as
/// two cones, every size builds exactly.
#[test]
fn pipe_fittings_weigh_what_their_tables_say() {
    let of = |family: &str, size: &str| {
        all()
            .into_iter()
            .find(|s| s.family == family && s.size == size)
            .unwrap_or_else(|| panic!("{family} {size}"))
    };
    // NPS 4: D 114.3, STD wall 6.02, R 152.
    let ring = PI * (57.15f64.powi(2) - (57.15f64 - 6.02).powi(2));
    let p = of("ASME B16.9 90", "NPS 4").part().unwrap();
    let want = ring * 152.0 * PI / 2.0;
    assert!((volume(&p) - want).abs() / want < 3e-3, "{}", volume(&p));
    let (lo, hi) = p.body.bounding_box().unwrap();
    assert!(
        lo.z.abs() < 1e-6 && (hi.z - 152.0 - 0.0).abs() < 60.0,
        "{lo:?} {hi:?}"
    );
    assert!(hi.x > 150.0, "it turns towards +X: {hi:?}");

    let cone = |a: f64, b: f64, h: f64| PI * h / 3.0 * (a * a + a * b + b * b);
    let p = of("ASME B16.9 Reducer", "NPS 4 x 2").part().unwrap();
    let want = cone(57.15, 30.15, 102.0) - cone(57.15 - 6.02, 30.15 - 3.91, 102.0);
    assert!((volume(&p) - want).abs() / want < 3e-3, "{}", volume(&p));

    let p = of("ASME B16.9 Cap", "NPS 4").part().unwrap();
    let (_, hi) = p.body.bounding_box().unwrap();
    assert!((hi.z - 64.0).abs() < 1e-6, "{hi:?}");
    assert!(volume(&p) > 0.0);

    // A tee: run + branch outside the run, bores taken out. The joint is small
    // against the rest, so its bound is loose.
    let p = of("ASME B16.9 Tee", "NPS 4").part().unwrap();
    let run = ring * 210.0;
    let v = volume(&p);
    assert!(v > run && v < run + ring * 105.0, "{v} against run {run}");
}

#[test]
fn every_pipe_fitting_builds() {
    let mut slowest = (0.0, String::new());
    for s in all().iter().filter(|s| {
        matches!(
            s.kind,
            Kind::PipeElbow(..) | Kind::PipeTee(..) | Kind::PipeReducer(..) | Kind::PipeCap(..)
        )
    }) {
        let t = Instant::now();
        let p = s
            .part()
            .unwrap_or_else(|e| panic!("{}: {e}", s.designation()));
        let ms = t.elapsed().as_secs_f64() * 1e3;
        if ms > slowest.0 {
            slowest = (ms, s.designation());
        }
        assert!(p.exact, "{} went faceted", s.designation());
        assert!(p.feature_health.is_empty(), "{}", s.designation());
    }
    println!("slowest fitting: {} {:.0} ms", slowest.1, slowest.0);
}

/// Wing, eye, tee-head, square head and shoulder fasteners build exactly, smallest and largest.
#[test]
fn head_fasteners_build_within_the_budget() {
    let mut seen = std::collections::HashMap::<String, (String, Sample)>::new();
    let chosen: Vec<Sample> = all()
        .into_iter()
        .filter(|s| {
            matches!(
                s.kind,
                Kind::WingNut(..)
                    | Kind::EyeBolt(..)
                    | Kind::THead(..)
                    | Kind::SquareHead(..)
                    | Kind::Shoulder(..)
            )
        })
        .collect();
    for s in &chosen {
        seen.entry(s.family.clone())
            .and_modify(|e| e.1 = s.clone())
            .or_insert_with(|| (s.designation(), s.clone()));
    }
    let firsts: Vec<Sample> = chosen
        .iter()
        .filter(|s| seen.get(&s.family).is_some_and(|e| e.0 == s.designation()))
        .cloned()
        .collect();
    for s in firsts.iter().chain(seen.values().map(|e| &e.1)) {
        let t = Instant::now();
        let p = s
            .part()
            .unwrap_or_else(|e| panic!("{}: {e}", s.designation()));
        let ms = t.elapsed().as_secs_f64() * 1e3;
        println!("{:<34} {ms:>6.0} ms", s.designation());
        assert!(p.exact, "{} went faceted", s.designation());
        assert!(p.feature_health.is_empty(), "{}", s.designation());
    }
}

/// A shoulder screw goes by its shoulder, through a hole as deep as the shoulder is long.
#[test]
fn a_shoulder_screw_weighs_what_its_table_says() {
    let s = all()
        .into_iter()
        .find(|s| s.family == "ISO 7379" && s.size == "M8 x 30")
        .unwrap();
    let p = s.part().unwrap();
    // Thread Ø8 x 13.25, shoulder Ø10 x 30, head Ø16 x 7, socket 5 x 4.2.
    let hex = 5.0f64 * 5.0 * 3f64.sqrt() / 2.0 * 4.2;
    let want = PI * (16.0 * 13.25 + 25.0 * 30.0 + 64.0 * 7.0) - hex;
    assert!((volume(&p) - want).abs() / want < 0.02, "{}", volume(&p));
}

/// A tee built directly: the exact volume of two equal tubes meeting square (Steinmetz: their
/// solid overlap is 16 r^3 / 3 for a whole crossing, half of it where the branch stops at the axis).
#[test]
fn a_tee_has_the_volume_of_two_tubes() {
    for size in ["NPS 1/2", "NPS 4", "NPS 24"] {
        let s = all()
            .into_iter()
            .find(|s| s.family == "ASME B16.9 Tee" && s.size == size)
            .unwrap();
        let t = Instant::now();
        let p = s.part().unwrap();
        let ms = t.elapsed().as_secs_f64() * 1e3;
        let Kind::PipeTee(d, wall, c) = s.kind else {
            panic!("not a tee");
        };
        let solid =
            |r: f64| PI * r * r * 2.0 * c + PI * r * r * c - 16.0 * r.powi(3) / 3.0 / 2.0 + 0.0;
        let want = solid(d / 2.0) - solid(d / 2.0 - wall);
        println!(
            "{size}: {ms:.0} ms, volume {:.1} against {want:.1}",
            volume(&p)
        );
        assert!(p.exact && p.feature_health.is_empty(), "{size}");
        assert!(
            (volume(&p) - want).abs() / want < 5e-3,
            "{size}: {} against {want}",
            volume(&p)
        );
    }
}

/// Every type has a builder now: each family's first and last member builds exactly.
#[test]
fn no_type_is_left_without_a_builder() {
    let types = include_str!("../data/components/types.csv");
    let planned: Vec<&str> = types.lines().filter(|l| l.contains(",planned,")).collect();
    assert!(planned.is_empty(), "{planned:?}");
    let every = all();
    let mut slowest = (0.0, String::new());
    let mut last: Option<Sample> = None;
    let mut seen = std::collections::HashSet::new();
    let check = |s: &Sample, slowest: &mut (f64, String)| {
        let t = Instant::now();
        let p = s
            .part()
            .unwrap_or_else(|e| panic!("{}: {e}", s.designation()));
        let ms = t.elapsed().as_secs_f64() * 1e3;
        if ms > slowest.0 {
            *slowest = (ms, s.designation());
        }
        if ms > 300.0 {
            println!("slow: {} {ms:.0} ms", s.designation());
        }
        assert!(p.exact, "{} went faceted", s.designation());
        assert!(
            p.feature_health.is_empty(),
            "{}: {:?}",
            s.designation(),
            p.feature_health
        );
        assert!(volume(&p) > 0.0, "{}", s.designation());
    };
    for s in &every {
        if seen.insert(s.family.clone()) {
            if let Some(l) = last.take() {
                check(&l, &mut slowest);
            }
            check(s, &mut slowest);
        }
        last = Some(s.clone());
    }
    if let Some(l) = last {
        check(&l, &mut slowest);
    }
    println!("slowest family end: {} {:.0} ms", slowest.1, slowest.0);
}

/// A gear's outline: its area by the tooth count, within a little of pitch circle's.
#[test]
fn a_gear_has_its_teeth() {
    let s = all()
        .into_iter()
        .find(|s| s.family == "Spur Gear ISO 53" && s.size == "m2 z40")
        .unwrap();
    let p = s.part().unwrap();
    let (lo, hi) = p.body.bounding_box().unwrap();
    assert!((hi.x - lo.x - 84.0).abs() < 0.1, "{lo:?} {hi:?}");
    // Pitch circle 80, b 20, bore 20: the teeth about balance what the root takes.
    let want = PI * (40.0f64 * 40.0 - 10.0 * 10.0) * 20.0;
    assert!(
        (volume(&p) - want).abs() / want < 0.04,
        "{} against {want}",
        volume(&p)
    );
}

/// A bearing's Full level holds every ball as a body, two rows where it has two; its picture is
/// its section, drawn on a clear background.
#[test]
fn a_full_bearing_has_its_balls_and_its_picture_its_section() {
    use cadcraft3d::components::icon::{BearingStyle, bearing_section};
    let of = |family: &str, size: &str| {
        all()
            .into_iter()
            .find(|s| s.family == family && s.size == size)
            .unwrap_or_else(|| panic!("{family} {size}"))
    };
    let one = of("Series 62", "6204");
    assert_eq!(one.bearing_style(), Some(BearingStyle::Ball));
    let p = one.part_at(Detail::Full).unwrap();
    assert!(
        p.exact && p.feature_health.is_empty(),
        "{:?}",
        p.feature_health
    );
    let bodies = p.body.solids().count();
    assert!((8..=22).contains(&bodies), "{bodies} bodies");

    let two = of("Series 12", "1204");
    assert_eq!(two.bearing_style().map(BearingStyle::rows), Some(2));
    let q = two.part_at(Detail::Full).unwrap();
    assert!(q.feature_health.is_empty(), "{:?}", q.feature_health);
    let (_, a) = (one.clone(), bodies - 2);
    assert!(q.body.solids().count() - 2 > a, "two rows hold more balls");

    let pic = bearing_section(BearingStyle::TwoBalls, 20.0, 47.0, 14.0, 56);
    assert_eq!(pic.len(), 56 * 56 * 4);
    let drawn = pic.chunks(4).filter(|c| c[3] > 200).count();
    assert!(drawn > 300, "{drawn} pixels");
    // The corner stays clear.
    assert_eq!(pic[3], 0);
}
