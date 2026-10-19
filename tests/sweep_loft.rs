//! Sweep and Loft with other features (`docs/spec/5.1-sweep.md`,
//! `docs/spec/5.2-loft.md`): fillet, shell and pattern after them, and the
//! measured volumes printed for the report (`--nocapture`).

mod common;
use common::*;

use cadcraft3d::part::{
    LoftFeature, LoftSection, PathRef, PatternDirection, PatternFeature, PatternKind, PullRef,
    SweepFeature, WorkDef, WorkFeature, WorkPlaneDef,
};
use cadcraft3d::sketch::Sketch;

fn sk(part: &mut PartDocument, plane: OriginPlane, draw: impl FnOnce(&mut Sketch)) -> usize {
    let k = part
        .create_sketch(SketchPlacement::on_origin_plane(plane), &opts())
        .unwrap();
    draw(&mut part.sketches[k].sketch);
    k
}

fn at_height(part: &mut PartDocument, z: Real, draw: impl FnOnce(&mut Sketch)) -> usize {
    let wp = part
        .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
            base: PlaneRef::Origin(OriginPlane::XY),
            distance: z,
        })))
        .unwrap();
    let k = part
        .create_sketch(
            SketchPlacement {
                plane: PlaneRef::Work(wp),
                ..Default::default()
            },
            &opts(),
        )
        .unwrap();
    draw(&mut part.sketches[k].sketch);
    k
}

fn profiles(part: &PartDocument, k: usize) -> Vec<ProfileRef> {
    region_profiles(&part.sketches[k].sketch.regions(), |_| true)
}

fn path(part: &PartDocument, k: usize) -> PathRef {
    let ps = &part.sketches[k];
    PathRef::Sketch {
        sketch: ps.id.clone(),
        curves: ps.sketch.entities().map(|(id, _)| id).collect(),
    }
}

fn section(part: &PartDocument, k: usize) -> LoftSection {
    let r = part.sketches[k].sketch.regions();
    LoftSection::Profile {
        sketch: part.sketches[k].id.clone(),
        profile: ProfileRef::of(&r[0], r[0].interior_point()),
    }
}

fn square(s: &mut Sketch, w: Real) {
    s.add_rectangle_two_point(
        Point2::new(-w / 2.0, -w / 2.0),
        Point2::new(w / 2.0, w / 2.0),
    )
    .unwrap();
}

/// A 20×20 square on XY swept 30 up the Z axis (a prism).
fn swept_prism() -> PartDocument {
    let mut part = PartDocument::default();
    let a = sk(&mut part, OriginPlane::XY, |s| square(s, 20.0));
    let b = sk(&mut part, OriginPlane::XZ, |s| {
        s.add_line(Point2::new(0.0, 0.0), Point2::new(0.0, 30.0));
    });
    let def = SweepFeature::new(profiles(&part, a), path(&part, b));
    part.add_sweep(a, def).unwrap();
    part
}

#[test]
fn sweep_then_fillet() {
    let mut part = swept_prism();
    check(&part, 12000.0, 1e-4);
    let edges = edges_where(&part, |h| {
        (h[0] - 10.0).abs() < 1e-6 && (h[1] - 10.0).abs() < 1e-6
    });
    assert_eq!(edges.len(), 1, "{edges:?}");
    part.add_blend(BlendFeature::fillet(edges, 2.0)).unwrap();
    let want = 12000.0 - (4.0 - PI) * 30.0;
    eprintln!("sweep + fillet: {} vs {want}", volume(&part));
    check(&part, want, 1e-3);
}

#[test]
fn sweep_then_shell() {
    let mut part = swept_prism();
    let end = face_where(&part, |p| (p.z - 30.0).abs() < 1e-6);
    assert_eq!(end, "Sweep1:End");
    shell(
        &mut part,
        &[(&end, [0.0, 0.0, 30.0])],
        2.0,
        ShellDirection::Inside,
    );
    let want = 12000.0 - 16.0 * 16.0 * 28.0;
    eprintln!("sweep + shell: {} vs {want}", volume(&part));
    check(&part, want, 1e-3);
}

#[test]
fn loft_then_fillet() {
    let mut part = PartDocument::default();
    let a = sk(&mut part, OriginPlane::XY, |s| square(s, 20.0));
    let b = at_height(&mut part, 30.0, |s| square(s, 20.0));
    part.add_loft(LoftFeature::new(vec![section(&part, a), section(&part, b)]))
        .unwrap();
    check(&part, 12000.0, 1e-4);
    let edges = edges_where(&part, |h| {
        (h[0] - 10.0).abs() < 1e-6 && (h[1] - 10.0).abs() < 1e-6
    });
    assert_eq!(edges.len(), 1, "{edges:?}");
    part.add_blend(BlendFeature::fillet(edges, 2.0)).unwrap();
    let want = 12000.0 - (4.0 - PI) * 30.0;
    eprintln!("loft + fillet: {} vs {want}", volume(&part));
    check(&part, want, 1e-3);
}

#[test]
fn pattern_of_a_sweep() {
    let mut part = plate(60.0, 20.0, 10.0);
    // A rod r = 2 standing on the plate at (10, 10), 15 high.
    let a = at_height(&mut part, 10.0, |s| {
        s.add_circle(Point2::new(10.0, 10.0), 2.0).unwrap();
    });
    let b = sk(&mut part, OriginPlane::XZ, |s| {
        s.add_line(Point2::new(10.0, 10.0), Point2::new(10.0, 25.0));
    });
    // The path lies at y = 0, the rod at y = 10: the profile keeps its
    // place relative to the path.
    let mut def = SweepFeature::new(profiles(&part, a), path(&part, b));
    def.operation = Operation::Join;
    let id = part.add_sweep(a, def).unwrap();
    let rod = PI * 4.0 * 15.0;
    check(&part, 12000.0 + rod, 1e-3);
    let mut p = PatternFeature::rectangular();
    p.features = vec![id];
    p.kind = PatternKind::Rectangular {
        first: PatternDirection {
            dir: Some(PullRef::Plane(PlaneRef::Origin(OriginPlane::YZ))),
            count: 3,
            spacing: 20.0,
            ..PatternDirection::new()
        },
        second: None,
    };
    part.add_pattern(p).unwrap();
    eprintln!(
        "sweep pattern: {} vs {}",
        volume(&part),
        12000.0 + 3.0 * rod
    );
    check(&part, 12000.0 + 3.0 * rod, 1e-3);
}

#[test]
fn sweep_survives_a_project_round_trip() {
    let part = swept_prism();
    let mut project = Project::new("Sweep");
    project.add_document("Part1", DocumentData::Part(part));
    let dir = std::env::temp_dir().join(format!("cadcraft-sweep-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("sweep.3dcadcraft");
    project.save(&file).unwrap();
    let back = Project::load(&file).unwrap();
    let DocumentData::Part(mut q) = back.documents()[0].data.clone() else {
        panic!("not a part");
    };
    q.regenerate().unwrap();
    check(&q, 12000.0, 1e-4);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn closed_loop_loft_ring() {
    let mut part = PartDocument::default();
    let xz = |p: &mut PartDocument, x: Real| {
        sk(p, OriginPlane::XZ, |s| {
            s.add_circle(Point2::new(x, 0.0), 4.0).unwrap();
        })
    };
    let yz = |p: &mut PartDocument, y: Real| {
        sk(p, OriginPlane::YZ, |s| {
            s.add_circle(Point2::new(y, 0.0), 4.0).unwrap();
        })
    };
    let ks = [
        xz(&mut part, 30.0),
        yz(&mut part, 30.0),
        xz(&mut part, -30.0),
        yz(&mut part, -30.0),
    ];
    let mut def = LoftFeature::new(ks.iter().map(|&k| section(&part, k)).collect());
    def.closed_loop = true;
    match part.add_loft(def) {
        Ok(_) => {
            let torus = PI * 16.0 * 2.0 * PI * 30.0;
            eprintln!("closed loop: {} vs torus {torus}", volume(&part));
            check(&part, torus, 0.25);
        }
        Err(e) => {
            eprintln!("closed loop refused: {e}");
            assert!(e.to_string().contains("kernel"), "{e}");
        }
    }
}

#[test]
fn duct_loft_shelled_open_at_both_ends() {
    // FreeCAD #6196: a square 40 → circle Ø30 duct, shell t = 2 with both
    // ends open: a valid duct or a clear refusal, never a broken body.
    let mut part = PartDocument::default();
    let a = sk(&mut part, OriginPlane::XY, |s| square(s, 40.0));
    let b = at_height(&mut part, 40.0, |s| {
        s.add_circle(Point2::new(0.0, 0.0), 15.0).unwrap();
    });
    part.add_loft(LoftFeature::new(vec![section(&part, a), section(&part, b)]))
        .unwrap();
    let full = volume(&part);
    let mut def = ShellFeature::new(2.0);
    def.remove = vec![
        face_ref(&part, "Loft1:Start", [0.0, 0.0, 0.0]),
        face_ref(&part, "Loft1:End", [0.0, 0.0, 40.0]),
    ];
    match part.add_shell(def) {
        Ok(_) => {
            assert!(part.feature_health.is_empty(), "{:?}", part.feature_health);
            for (sid, _) in part.body.solids() {
                part.body.validate_solid(sid).unwrap();
            }
            let v = volume(&part);
            eprintln!("duct shell: {v} of {full}");
            assert!(v > 0.05 * full && v < 0.5 * full, "{v} of {full}");
        }
        Err(e) => {
            eprintln!("duct shell refused: {e}");
            assert!(
                part.features
                    .iter()
                    .all(|f| !matches!(f, Feature::Shell(_)))
            );
        }
    }
}
