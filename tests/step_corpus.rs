//! STEP files of known shape, read through our Import (spec 7.7a): SolidCraft's corpora
//! (`tests/step_corpus/`, README there). Each file goes through `ImportOp::read` — the kernel's
//! reader and our conversion to named bodies — and is checked against its exact volume, box and
//! body count (and area for open shells).
//!
//! Expected values come from the files' generators (formulas), never from a run of ours. A case
//! that cannot pass yet is listed in `tests/step_corpus/known_failures.txt` with the reason; a
//! listed case that starts passing fails the run, so the list stays true.
//!
//! The external corpus (`external-corpus.toml`: NIST PMI files from several exporters, hash
//! checked) needs a download: `external_corpus` is ignored by default and reads the files from
//! `CADCRAFT3D_EXTERNAL_CORPUS` (the folder the NIST archive was unpacked into).

use std::path::{Path, PathBuf};

use cadcraft3d::kernel::Real;
use cadcraft3d::part::import::ImportOp;

const PI: Real = std::f64::consts::PI;

/// What a file must give.
struct Case {
    file: &'static str,
    /// Bodies.
    bodies: usize,
    /// Total volume of the solids (mm³), or the total area of an open shell (mm²).
    amount: Amount,
    /// The box of everything (min x y z, max x y z), when known.
    bbox: Option<[Real; 6]>,
}

enum Amount {
    Volume(Real),
    Area(Real),
    /// Only that it reads into closed solids.
    Solid,
    /// Only that it reads (a single face of a surface: no volume to check).
    Reads,
}

use Amount::*;

fn cases() -> Vec<Case> {
    let c = |file, bodies, amount, bbox| Case {
        file,
        bodies,
        amount,
        bbox,
    };
    let cyl = Some([-10.0, -10.0, 0.0, 10.0, 10.0, 30.0]);
    vec![
        // step-conformance: (volume and box from the generator's own profile).
        c("conformance/cone_point.step", 1, Volume(1000.0 * PI), cyl),
        c(
            "conformance/cone_point_vertex.step",
            1,
            Volume(1000.0 * PI),
            cyl,
        ),
        c("conformance/cylinder.step", 1, Volume(9424.778), cyl),
        c("conformance/cylinder_inch.step", 1, Volume(9424.778), cyl),
        c("conformance/cylinder_nurbs.step", 1, Volume(9424.778), cyl),
        c(
            "conformance/cylinder_placed.step",
            1,
            Volume(9424.778),
            Some([31.835, -33.165, 1.835, 65.485, 0.485, 35.485]),
        ),
        c("conformance/cylinder_seam.step", 1, Volume(9424.778), cyl),
        c(
            "conformance/cylinder_twice.step",
            2,
            Volume(18849.556),
            Some([-10.0, -10.0, 0.0, 90.0, 10.0, 30.0]),
        ),
        c(
            "conformance/dome.step",
            1,
            Volume(16755.161),
            Some([-20.0, -20.0, 0.0, 20.0, 20.0, 20.0]),
        ),
        c(
            "conformance/dome_nurbs.step",
            1,
            Volume(16755.161),
            Some([-20.0, -20.0, 0.0, 20.0, 20.0, 20.0]),
        ),
        c(
            "conformance/far_cylinder.step",
            1,
            Volume(9424.778),
            Some([99990.0, -10.0, 0.0, 100010.0, 10.0, 30.0]),
        ),
        c(
            "conformance/grooved_shaft.step",
            1,
            Volume(153532.251),
            Some([-10.0, -10.0, 0.0, 10.0, 10.0, 501.0]),
        ),
        c(
            "conformance/plate_100_holes.step",
            1,
            Volume(71588.321),
            Some([0.0, 0.0, 0.0, 100.0, 100.0, 10.0]),
        ),
        c(
            "conformance/sheet_disc.step",
            1,
            Volume(193396443.755),
            Some([-1800.0, -1800.0, 0.0, 1800.0, 1800.0, 19.0]),
        ),
        c(
            "conformance/tiny_pin.step",
            1,
            Volume(PI / 8.0),
            Some([-0.25, -0.25, 0.0, 0.25, 0.25, 2.0]),
        ),
        c(
            "conformance/torus_ring_nurbs.step",
            1,
            Volume(4934.802),
            Some([-25.0, -25.0, 0.0, 25.0, 25.0, 5.0]),
        ),
        c(
            "conformance/washer.step",
            1,
            Volume(3534.292),
            Some([-20.0, -20.0, 0.0, 20.0, 20.0, 3.0]),
        ),
        // step-regressions (shapes from the README's formulas).
        c("regressions/rod_doubled_edge.step", 1, Solid, None),
        c(
            "regressions/tiny_rounds_coarse_tolerance.step",
            1,
            // A quarter round 1 mm long, an eighth of a ball, a quarter torus tube on a quarter
            // ring of radius 2, all of radius 0.05.
            Area({
                let (r, ring, half) = (0.05, 2.0, std::f64::consts::FRAC_PI_2);
                half * r + 4.0 * PI * r * r / 8.0 + half * r * (ring * half + r)
            }),
            None,
        ),
        c(
            "regressions/cylinder_spline_rims.step",
            1,
            Volume(PI * 100.0 * 5.0),
            None,
        ),
        c(
            "regressions/cylinder_rim_off_its_wall.step",
            1,
            Volume(PI * 100.0 * 5.0),
            None,
        ),
        c(
            "regressions/block_edge_off_its_face.step",
            1,
            Volume(20.0 * 30.0 * 10.0),
            Some([0.0, 0.0, 0.0, 20.0, 30.0, 10.0]),
        ),
        // Torus R 12, r 2: 2π² R r².
        c(
            "regressions/torus_patch_and_rest.step",
            1,
            Volume(2.0 * PI * PI * 12.0 * 4.0),
            None,
        ),
        c(
            "regressions/torus_patch_and_rest_revolved.step",
            1,
            Volume(2.0 * PI * PI * 12.0 * 4.0),
            None,
        ),
        // A 30 × 30 × 10 box less a Ø14 bore, the bore's top rim rounded r 1: the round takes
        // (1 − π/4) r² revolved about the axis at its centroid's radius 7 + (10 − 3π)/(12 − 3π) r.
        c(
            "regressions/hole_round_revolved.step",
            1,
            Volume({
                let spandrel = 1.0 - PI / 4.0;
                let at = 7.0 + (10.0 - 3.0 * PI) / (12.0 - 3.0 * PI);
                9000.0 - PI * 49.0 * 10.0 - 2.0 * PI * at * spandrel
            }),
            None,
        ),
        // One cylinder face of radius 3 from a circle at z = 3 down to a helical groove (a
        // thread's crest): it must come in with its face.
        c("regressions/thread_crest_band.step", 1, Reads, None),
        // The tetrahedron (0,0,0) (10,0,0) (0,10,0) (0,0,10).
        c(
            "regressions/net_steps_back_within_tolerance.step",
            1,
            Volume(1000.0 / 6.0),
            Some([0.0, 0.0, 0.0, 10.0, 10.0, 10.0]),
        ),
    ]
}

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/step_corpus")
}

/// Checks one case; `Err` says what differs.
fn check(c: &Case) -> Result<(), String> {
    let text = std::fs::read_to_string(dir().join(c.file)).map_err(|e| e.to_string())?;
    let (solids, _warnings) = ImportOp::new(c.file, text.as_str()).read("Import1")?;
    if solids.len() != c.bodies {
        return Err(format!("{} bodies, want {}", solids.len(), c.bodies));
    }
    let mut volume = 0.0;
    let mut area = 0.0;
    let mut lo = [Real::INFINITY; 3];
    let mut hi = [Real::NEG_INFINITY; 3];
    for s in &solids {
        let m = s.measure_full()?;
        if !s.is_open() {
            volume += m.volume;
        }
        area += m.area;
        for k in 0..3 {
            lo[k] = lo[k].min(m.bbox.0[k]);
            hi[k] = hi[k].max(m.bbox.1[k]);
        }
    }
    let rel = |a: Real, b: Real| (a - b).abs() / b.abs().max(1e-12);
    match c.amount {
        Volume(v) => {
            if solids.iter().any(|s| s.is_open()) {
                return Err("an open body where a solid was expected".into());
            }
            if rel(volume, v) > 1e-3 {
                return Err(format!("volume {volume} vs {v}"));
            }
        }
        Area(a) => {
            if rel(area, a) > 1e-3 {
                return Err(format!("area {area} vs {a}"));
            }
        }
        Reads => {}
        Solid => {
            if solids.iter().any(|s| s.is_open()) || !(volume > 0.0) {
                return Err(format!("not a closed solid (volume {volume})"));
            }
        }
    }
    if let Some(b) = c.bbox {
        let size = ((b[3] - b[0]).powi(2) + (b[4] - b[1]).powi(2) + (b[5] - b[2]).powi(2)).sqrt();
        for k in 0..3 {
            if (lo[k] - b[k]).abs() > size * 1e-3 || (hi[k] - b[k + 3]).abs() > size * 1e-3 {
                return Err(format!("box {lo:?} {hi:?} vs {b:?}"));
            }
        }
    }
    Ok(())
}

/// `file  reason` lines of the cases that cannot pass yet.
fn known_failures() -> Vec<String> {
    std::fs::read_to_string(dir().join("known_failures.txt"))
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_whitespace().next().map(str::to_owned))
        .collect()
}

#[test]
fn step_files_of_known_shape_import_exactly() {
    let known = known_failures();
    let mut problems = Vec::new();
    for c in cases() {
        let started = std::time::Instant::now();
        let r = check(&c);
        eprintln!(
            "{:55} {:>6} ms  {}",
            c.file,
            started.elapsed().as_millis(),
            r.as_ref().err().map_or("ok", |e| e.as_str())
        );
        match (r, known.iter().any(|k| k == c.file)) {
            (Ok(()), true) => problems.push(format!(
                "{}: passes now; take it out of known_failures.txt",
                c.file
            )),
            (Err(e), false) => problems.push(format!("{}: {e}", c.file)),
            _ => {}
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Every file of the corpus folders has a case (a file added without expectations is a mistake).
#[test]
fn every_corpus_file_has_a_case() {
    let cases: Vec<&str> = cases().iter().map(|c| c.file).collect();
    for sub in ["conformance", "regressions"] {
        for e in std::fs::read_dir(dir().join(sub)).unwrap() {
            let name = e.unwrap().file_name().to_string_lossy().into_owned();
            if name.ends_with(".step") {
                let rel = format!("{sub}/{name}");
                assert!(cases.contains(&rel.as_str()), "{rel} has no case");
            }
        }
    }
}

/// One `[[file]]` of `external-corpus.toml` (a small reader of the keys used there).
#[derive(Default, Debug)]
struct External {
    name: String,
    path: String,
    bodies: Option<usize>,
    volume: Option<Real>,
    area: Option<Real>,
}

fn external_files() -> Vec<External> {
    let text = std::fs::read_to_string(dir().join("external-corpus.toml")).unwrap();
    let mut out: Vec<External> = Vec::new();
    let mut in_file = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with("[[") {
            in_file = line == "[[file]]";
            if in_file {
                out.push(External::default());
            }
            continue;
        }
        let (Some(f), Some((k, v))) = (out.last_mut().filter(|_| in_file), line.split_once('='))
        else {
            continue;
        };
        let v = v.trim().trim_matches('"');
        match k.trim() {
            "name" => f.name = v.into(),
            "path" => f.path = v.into(),
            "bodies" => f.bodies = v.parse().ok(),
            "volume_mm3" => f.volume = v.parse().ok(),
            "area_mm2" => f.area = v.parse().ok(),
            _ => {}
        }
    }
    out
}

/// The NIST PMI files from several exporters (download: see `external-corpus.toml`; the volumes
/// and areas there were measured by SolidCraft's importer, so they are a cross-check, within
/// 0.5 %).
#[test]
#[ignore = "needs the external corpus: set CADCRAFT3D_EXTERNAL_CORPUS to the unpacked NIST archive"]
fn external_corpus() {
    let Ok(root) = std::env::var("CADCRAFT3D_EXTERNAL_CORPUS") else {
        panic!("set CADCRAFT3D_EXTERNAL_CORPUS");
    };
    let files = external_files();
    assert!(files.len() >= 10, "{}", files.len());
    let mut problems = Vec::new();
    for f in files {
        let path = Path::new(&root).join(&f.path);
        let Ok(bytes) = std::fs::read(&path) else {
            problems.push(format!("{}: missing {}", f.name, path.display()));
            continue;
        };
        let text = String::from_utf8_lossy(&bytes);
        let started = std::time::Instant::now();
        let r = ImportOp::new(&f.path, text.as_ref()).read("Import1");
        let ms = started.elapsed().as_millis();
        let solids = match r {
            Ok((s, _)) => s,
            Err(e) => {
                problems.push(format!("{}: {e}", f.name));
                continue;
            }
        };
        let (mut v, mut a) = (0.0, 0.0);
        for s in &solids {
            if let Ok(m) = s.measure_full() {
                v += m.volume;
                a += m.area;
            }
        }
        eprintln!(
            "{:24} {ms:>7} ms  bodies {}  volume {v:.3}  area {a:.3}",
            f.name,
            solids.len()
        );
        let off = |got: Real, want: Option<Real>| want.is_some_and(|w| (got - w).abs() > 5e-3 * w);
        if f.bodies.is_some_and(|b| b != solids.len()) || off(v, f.volume) || off(a, f.area) {
            problems.push(format!(
                "{}: bodies {} volume {v} area {a}, want {:?} {:?} {:?}",
                f.name,
                solids.len(),
                f.bodies,
                f.volume,
                f.area
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
