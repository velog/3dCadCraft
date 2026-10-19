//! Old project files keep opening (`docs/FILE_FORMAT.md` §7): every file in
//! `tests/compat/r<N>/` was saved by the build of format revision N and must
//! load, regenerate and give the same bodies (volume, faces, features,
//! sketches) as recorded in `tests/compat/expected.json` when it was added.
//!
//! Rules: never edit or delete a fixture or its expected values to make a
//! change pass — a failure means old files break, so add a migration
//! (`document::format::load_with`) instead. When `FORMAT_REVISION` is
//! bumped, add files using the new things under `r<new>/` and record them:
//! `CADCRAFT_WRITE_COMPAT=1 cargo test --test compat` (writes only entries
//! that are missing, never changes existing ones).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cadcraft3d::document::{DocumentData, Project};
use cadcraft3d::kernel::{Point3, Real, TessellationOptions};
use cadcraft3d::part::blend::edge_ref;
use cadcraft3d::part::{
    BlendFeature, EdgeRef, ExtrudeFeature, OriginPlane, PartDocument, ProfileRef, SketchOptions,
    SketchPlacement, VariableRadius,
};
use cadcraft3d::sketch::{PatternDirection, Point2};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Expect {
    volume: Real,
    faces: usize,
    features: usize,
    sketches: usize,
    exact: bool,
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/compat")
}

fn volume(part: &PartDocument) -> Real {
    let opts = TessellationOptions::default();
    part.body
        .solids()
        .map(|(sid, _)| {
            let m = part.body.tessellate_solid(sid, &opts).unwrap();
            m.faces
                .triangles
                .iter()
                .map(|t| {
                    let [p, q, s] = t.map(|i| m.faces.positions[i as usize].coords);
                    p.dot(&q.cross(&s)) / 6.0
                })
                .sum::<Real>()
        })
        .sum()
}

fn measure(path: &Path) -> Result<Expect, String> {
    let mut project = Project::load(path).map_err(|e| e.to_string())?;
    let id = project
        .documents()
        .iter()
        .find(|d| matches!(d.data, DocumentData::Part(_)))
        .map(|d| d.id)
        .ok_or("no part")?;
    let Some(DocumentData::Part(part)) = project.document_mut(id).map(|d| &mut d.data) else {
        return Err("no part".into());
    };
    part.regenerate().map_err(|e| e.to_string())?;
    if !part.feature_health.is_empty() {
        return Err(format!("sick features: {:?}", part.feature_health));
    }
    Ok(Expect {
        volume: volume(part),
        faces: part.body.faces().count(),
        features: part.features.len(),
        sketches: part.sketches.len(),
        exact: part.exact,
    })
}

/// Revision 1 extras: the fillet types and sets, a pattern whose count and
/// spacing follow user parameters.
fn features_r1() -> PartDocument {
    let mut part = PartDocument::with_box("Box1", [60.0, 40.0, 20.0]).unwrap();
    let edges = |part: &PartDocument, keep: &dyn Fn([Real; 3]) -> bool| -> Vec<EdgeRef> {
        part.body
            .edges()
            .filter_map(|(e, _)| edge_ref(&part.body, e))
            .filter(|r| keep(r.hint))
            .collect()
    };
    // Variable radius on the front top edge (y = 0, z = 20).
    let front = edges(&part, &|h| h[1].abs() < 1e-6 && (h[2] - 20.0).abs() < 1e-6);
    part.add_blend(BlendFeature::variable_fillet(
        vec![VariableRadius {
            edge: front[0].clone(),
            start: [0.0, 0.0, 20.0],
            points: vec![[0.0, 1.0], [0.5, 3.0], [1.0, 2.0]],
        }],
        true,
    ))
    .unwrap();
    // Chord width on the back top edge (y = 40, z = 20).
    let back = edges(&part, &|h| {
        (h[1] - 40.0).abs() < 1e-6 && (h[2] - 20.0).abs() < 1e-6
    });
    part.add_blend(BlendFeature::chord_fillet(back, 2.0))
        .unwrap();
    // Two edge sets on the vertical edges at x = 60.
    let right = edges(&part, &|h| {
        (h[0] - 60.0).abs() < 1e-6 && h[2] > 1.0 && h[2] < 19.0
    });
    let mut sets = BlendFeature::fillet(vec![right[0].clone()], 2.0);
    sets.sets.push(cadcraft3d::part::FilletSet {
        edges: vec![right[1].clone()],
        radius: 4.0,
    });
    part.add_blend(sets).unwrap();
    // A sketch pattern driven by user parameters (count and spacing).
    let n = part.add_user_parameter(cadcraft3d::part::expr::Unit::Ul);
    part.set_parameter_expr(&n, "4").unwrap();
    let w = part.add_user_parameter(cadcraft3d::part::expr::Unit::Mm);
    part.set_parameter_expr(&w, "9 mm").unwrap();
    let opts = SketchOptions {
        auto_project_origin: false,
        ..Default::default()
    };
    let i = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts)
        .unwrap();
    let sk = &mut part.sketches[i].sketch;
    let a = sk.add_fixed_point(Point2::new(80.0, -10.0));
    let b = sk.add_fixed_point(Point2::new(90.0, -10.0));
    let axis = sk.add_line_between(a, b).unwrap();
    let c = sk.add_circle(Point2::new(80.0, 0.0), 2.0).unwrap();
    sk.rectangular_pattern(
        &[c],
        PatternDirection {
            line: axis,
            flip: false,
            count: 3,
            spacing: 10.0,
            fitted: false,
        },
        None,
    )
    .unwrap();
    let (pid, p) = sk.patterns().next().unwrap();
    let dim = *p
        .constraints
        .iter()
        .find(|&&k| sk.constraint(k).and_then(|x| x.value()).is_some())
        .unwrap();
    sk.dim_param_mut(dim).unwrap().expr = Some(w.clone());
    sk.set_pattern_count_exprs(pid, [Some(n.clone()), None])
        .unwrap();
    part.regenerate().unwrap();
    // A plain extrusion of the box-sized profile elsewhere, so the file
    // also has an extrude with a sketch.
    let j = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts)
        .unwrap();
    part.sketches[j]
        .sketch
        .add_rectangle_two_point(Point2::new(-30.0, 0.0), Point2::new(-10.0, 10.0))
        .unwrap();
    let r = part.sketches[j].sketch.regions();
    let mut ex = ExtrudeFeature::new(vec![ProfileRef::of(&r[0], r[0].interior_point())], 5.0);
    ex.operation = cadcraft3d::part::Operation::NewSolid;
    part.add_extrude(j, ex).unwrap();
    let _ = Point3::origin();
    part
}

/// Revision 2: a partial chamfer (15 of the 60 mm top front edge).
fn partial_r2() -> PartDocument {
    let mut part = PartDocument::with_box("Box1", [60.0, 40.0, 20.0]).unwrap();
    let front: Vec<EdgeRef> = part
        .body
        .edges()
        .filter_map(|(e, _)| edge_ref(&part.body, e))
        .filter(|r| r.hint[1].abs() < 1e-6 && (r.hint[2] - 20.0).abs() < 1e-6)
        .collect();
    let mut def = BlendFeature::chamfer(front, 2.0);
    def.partial = Some(cadcraft3d::part::PartialChamfer {
        start: [0.0, 0.0, 20.0],
        to_start: 10.0,
        length: 15.0,
    });
    part.add_blend(def).unwrap();
    part
}

#[test]
fn old_files_still_open() {
    let write = std::env::var_os("CADCRAFT_WRITE_COMPAT").is_some();
    let exp_path = root().join("expected.json");
    let mut expected: BTreeMap<String, Expect> = std::fs::read_to_string(&exp_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    if write {
        let f = root().join("r1/features_r1.3dcadcraft");
        if !f.exists() {
            let mut project = Project::new("FeaturesR1");
            project.add_document("Part1", DocumentData::Part(features_r1()));
            project.save(&f).unwrap();
        }
        let f = root().join("r2/partial_chamfer_r2.3dcadcraft");
        if !f.exists() {
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            let mut project = Project::new("PartialR2");
            project.add_document("Part1", DocumentData::Part(partial_r2()));
            project.save(&f).unwrap();
        }
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(root())
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .flat_map(|d| {
            std::fs::read_dir(d)
                .unwrap()
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("3dcadcraft"))
                .collect::<Vec<_>>()
        })
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no fixtures");
    let mut problems = Vec::new();
    let mut added = false;
    for f in &files {
        let key = f
            .strip_prefix(root())
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let got = match measure(f) {
            Ok(g) => g,
            Err(e) => {
                problems.push(format!("{key}: {e}"));
                continue;
            }
        };
        match expected.get(&key) {
            Some(want) => {
                let ok = (got.volume - want.volume).abs() <= 1e-6 * want.volume.abs().max(1.0)
                    && got.faces == want.faces
                    && got.features == want.features
                    && got.sketches == want.sketches
                    && got.exact == want.exact;
                println!(
                    "{key}: V {:.3} faces {} {}",
                    got.volume,
                    got.faces,
                    if ok { "ok" } else { "CHANGED" }
                );
                if !ok {
                    problems.push(format!("{key}: {got:?} vs recorded {want:?}"));
                }
            }
            None if write => {
                println!("{key}: recorded V {:.3}", got.volume);
                expected.insert(key, got);
                added = true;
            }
            None => problems.push(format!("{key}: not recorded (CADCRAFT_WRITE_COMPAT=1)")),
        }
    }
    if added {
        std::fs::write(&exp_path, serde_json::to_string_pretty(&expected).unwrap()).unwrap();
    }
    assert!(
        problems.is_empty(),
        "old files changed or fail:\n{}",
        problems.join("\n")
    );
}
