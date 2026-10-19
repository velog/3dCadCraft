//! Tests of history editing: End of Part, insertion, reorder, suppress,
//! delete, rename, and the build cache across them.

use super::super::*;
use super::DeleteOptions;
use crate::kernel::{Point3, Real, TessellationOptions};
use crate::sketch::Point2;
use std::f64::consts::PI;

const OPTS: SketchOptions = SketchOptions {
    auto_project_origin: false,
    auto_reference_edges: false,
    look_at_on_create: false,
    show_axes: false,
    show_triad: false,
    show_grid: false,
};

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

fn close(a: Real, b: Real) -> bool {
    (a - b).abs() <= 2e-3 * b.abs().max(1.0)
}

/// A 60 × 40 × 10 plate.
fn plate() -> PartDocument {
    PartDocument::with_box("Box1", [60.0, 40.0, 10.0]).unwrap()
}

/// A circle of radius `r` at `at` on the plate's top, cut through all
/// (`cut`) or joined `h` high.
fn circle_feature(
    part: &mut PartDocument,
    at: (Real, Real),
    r: Real,
    cut: bool,
    h: Real,
) -> String {
    let j = part
        .create_sketch(SketchPlacement::on_face("Box1:Top"), &OPTS)
        .unwrap();
    let c = part.sketches[j]
        .sketch
        .plane
        .project(&Point3::new(at.0, at.1, 10.0));
    part.sketches[j].sketch.add_circle(c, r).unwrap();
    let rg = part.sketches[j].sketch.regions();
    let mut ex = ExtrudeFeature::new(vec![ProfileRef::of(&rg[0], rg[0].interior_point())], h);
    if cut {
        ex.operation = Operation::Cut;
        ex.extent = Extent::ThroughAll;
        ex.direction = ExtentDirection::Flipped;
    } else {
        ex.operation = Operation::Join;
    }
    part.add_extrude(j, ex).unwrap()
}

fn edge(x: &str, y: &str, hint: [Real; 3]) -> EdgeRef {
    EdgeRef {
        faces: [format!("Box1:{x}"), format!("Box1:{y}")],
        hint,
    }
}

fn pattern_of(id: &str, count: u32, spacing: Real) -> PatternFeature {
    let mut rect = PatternFeature::rectangular();
    rect.features = vec![id.to_owned()];
    if let PatternKind::Rectangular { first, .. } = &mut rect.kind {
        first.dir = Some(PullRef::Line(LineRef::Origin(0)));
        first.count = count;
        first.spacing = spacing;
    }
    rect
}

const BOX: Real = 60.0 * 40.0 * 10.0;

fn hole_v(r: Real) -> Real {
    PI * r * r * 10.0
}

/// The cache gives what a cold rebuild gives.
fn assert_cold_same(part: &PartDocument) {
    let mut cold = part.clone();
    cold.build_cache = Default::default();
    cold.regenerate().unwrap();
    assert!(
        close(volume(&cold), volume(part)),
        "{} vs {}",
        volume(&cold),
        volume(part)
    );
    assert_eq!(cold.body.faces().count(), part.body.faces().count());
    assert_eq!(cold.feature_health, part.feature_health);
}

/// End of Part up and down: the volume at each position; features below
/// are rolled back and sketches made after it too.
#[test]
fn eop_up_and_down() {
    let mut part = plate();
    circle_feature(&mut part, (10.0, 10.0), 3.0, true, 1.0);
    circle_feature(&mut part, (40.0, 20.0), 4.0, false, 5.0);
    let full = BOX - hole_v(3.0) + PI * 16.0 * 5.0;
    assert!(close(volume(&part), full));
    let want = [BOX, BOX - hole_v(3.0), full];
    for at in [0usize, 1, 2, 3, 1, 2, 0, 3] {
        part.set_eop(at).unwrap();
        assert_eq!(part.eop_index(), at);
        let v = if at == 0 { 0.0 } else { want[at - 1] };
        assert!(
            close(volume(&part), v),
            "EOP {at}: {} vs {v}",
            volume(&part)
        );
        assert!(part.feature_health.is_empty(), "{:?}", part.feature_health);
        assert_cold_same(&part);
    }
    assert_eq!(part.eop, None, "at the end = no marker");
    part.set_eop(1).unwrap();
    assert!(part.is_rolled_back(1) && part.is_rolled_back(2) && !part.is_rolled_back(0));
    // The boss's sketch (made after Extrusion1) is below the marker.
    let boss_sketch = part
        .sketches
        .iter()
        .position(|s| s.id == "Sketch2")
        .unwrap();
    assert!(part.sketch_rolled_back(boss_sketch));
    assert!(!part.sketch_rolled_back(0));
    // Edit Feature's rollback composes with the marker: the smaller wins.
    part.rollback = Some(2);
    assert_eq!(part.build_limit(), 1);
    part.rollback = Some(0);
    assert_eq!(part.build_limit(), 0);
}

/// Every kind of feature is inserted at the End of Part: right above the
/// marker, the later feature stays after it, the marker moves down.
#[test]
fn insertion_at_eop_for_every_kind() {
    type Add = fn(&mut PartDocument) -> String;
    let kinds: Vec<(&str, Add)> = vec![
        ("extrude", |p| {
            circle_feature(p, (10.0, 10.0), 3.0, true, 1.0)
        }),
        ("revolve", |p| {
            let j = p
                .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XZ), &OPTS)
                .unwrap();
            p.sketches[j]
                .sketch
                .add_rectangle_two_point(Point2::new(80.0, 0.0), Point2::new(85.0, 10.0))
                .unwrap();
            let rg = p.sketches[j].sketch.regions();
            p.add_revolve(
                j,
                RevolveFeature::new(
                    vec![ProfileRef::of(&rg[0], rg[0].interior_point())],
                    AxisRef::Origin(2),
                ),
            )
            .unwrap()
        }),
        ("fillet", |p| {
            p.add_blend(BlendFeature::fillet(
                vec![edge("Front", "Right", [60.0, 0.0, 5.0])],
                3.0,
            ))
            .unwrap()
        }),
        ("chamfer", |p| {
            p.add_blend(BlendFeature::chamfer(
                vec![edge("Back", "Left", [0.0, 40.0, 5.0])],
                2.0,
            ))
            .unwrap()
        }),
        ("shell", |p| {
            let top = p.body.find_face("Box1:Bottom").unwrap();
            let mut def = ShellFeature::new(1.5);
            def.remove
                .push(shell::face_ref(&p.body, top, None).unwrap());
            p.add_shell(def).unwrap()
        }),
        ("draft", |p| {
            let mut def = DraftFeature::new(3f64.to_radians());
            def.pull = Some(PullRef::Plane(PlaneRef::Origin(OriginPlane::XY)));
            let f = p.body.find_face("Box1:Front").unwrap();
            def.faces
                .push(shell::face_ref(&p.body, f, Some(Point3::new(30.0, 0.0, 1.0))).unwrap());
            p.add_draft(def).unwrap()
        }),
        ("work", |p| {
            p.add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
                base: PlaneRef::Face("Box1:Top".into()),
                distance: 5.0,
            })))
            .unwrap()
        }),
        ("hole", |p| {
            let (i, ids) = p
                .create_hole_sketch("Box1:Top", &[Point3::new(20.0, 30.0, 10.0)])
                .unwrap();
            let mut def = HoleFeature::new(HoleSpec::simple(6.0, 5.0));
            def.sketch = p.sketches[i].id.clone();
            def.centers = ids.into_iter().map(HoleCenter::Point).collect();
            p.add_hole(def).unwrap()
        }),
        ("pattern", |p| {
            // A pattern needs a source before the marker: the box itself
            // as a solid pattern.
            let mut m = PatternFeature::mirror();
            m.solid_mode = true;
            m.operation = Operation::NewSolid;
            m.kind = PatternKind::Mirror {
                plane: Some(PlaneRef::Origin(OriginPlane::YZ)),
            };
            p.add_pattern(m).unwrap()
        }),
    ];
    for (what, add) in kinds {
        let mut part = plate();
        let late = circle_feature(&mut part, (50.0, 30.0), 2.0, true, 1.0);
        // A free sketch after everything (on an origin plane).
        let free = part
            .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &OPTS)
            .unwrap();
        assert_eq!(part.sketches[free].after_features, Some(2));
        part.set_eop(1).unwrap();
        assert!(part.sketch_rolled_back(free));
        let id = add(&mut part);
        let k = part.feature_index(&id).unwrap();
        assert_eq!(k, 1, "{what}: inserted above the marker");
        assert_eq!(part.eop_index(), 2, "{what}");
        assert_eq!(part.feature_index(&late), Some(2), "{what}");
        assert!(
            part.feature_health.is_empty(),
            "{what}: {:?}",
            part.feature_health
        );
        // Its sketch (if any) sits before it, the later sketch after.
        if let Some(s) = part.features[1].sketch() {
            let ps = part.sketches.iter().find(|p| p.id == s).unwrap();
            assert_eq!(ps.after_features, Some(1), "{what}");
        }
        let late_sk = part.features[2].sketch().unwrap().to_owned();
        let ps = part.sketches.iter().find(|p| p.id == late_sk).unwrap();
        assert!(ps.after_features.unwrap() <= 2, "{what}");
        assert_eq!(part.sketches[free].after_features, Some(3), "{what}");
        part.set_eop(3).unwrap();
        assert!(
            part.feature_health.is_empty(),
            "{what}: {:?}",
            part.feature_health
        );
        assert_cold_same(&part);
    }
}

/// A thread inserted at the marker into a hole made before it.
#[test]
fn thread_inserted_at_eop() {
    let mut part = plate();
    let (i, ids) = part
        .create_hole_sketch("Box1:Top", &[Point3::new(20.0, 20.0, 10.0)])
        .unwrap();
    let mut def = HoleFeature::new(HoleSpec::simple(6.8, 8.0));
    def.sketch = part.sketches[i].id.clone();
    def.centers = ids.into_iter().map(HoleCenter::Point).collect();
    part.add_hole(def).unwrap();
    circle_feature(&mut part, (50.0, 30.0), 2.0, true, 1.0);
    part.set_eop(2).unwrap();
    let wall = part.body.find_face("Hole1:Bore:1").unwrap();
    let face = shell::face_ref(&part.body, wall, None).unwrap();
    let mut spec = ThreadSpec::from_table(
        &standards::Standards::builtin(),
        standards::ISO_METRIC,
        "M8",
        true,
    )
    .unwrap();
    spec.modeled = false;
    let id = part.add_thread(ThreadFeature::new(face, spec)).unwrap();
    assert_eq!(part.feature_index(&id), Some(2));
    assert_eq!(part.eop_index(), 3);
    part.set_eop(4).unwrap();
    assert!(part.feature_health.is_empty(), "{:?}", part.feature_health);
    // The thread depends on the hole: suppressing the hole takes it along.
    let ids = part.suppress_feature(1).unwrap();
    assert!(ids.contains(&id), "{ids:?}");
}

/// A fillet inserted before a shell: the shell still builds, now with the
/// rounded corner.
#[test]
fn fillet_inserted_before_a_shell() {
    let mut part = plate();
    let top = part.body.find_face("Box1:Top").unwrap();
    let mut def = ShellFeature::new(2.0);
    def.remove
        .push(shell::face_ref(&part.body, top, None).unwrap());
    part.add_shell(def).unwrap();
    let shelled = volume(&part);
    let want = BOX - 56.0 * 36.0 * 8.0;
    assert!(close(shelled, want), "{shelled} vs {want}");
    part.set_eop(1).unwrap();
    let fid = part
        .add_blend(BlendFeature::fillet(
            vec![edge("Front", "Right", [60.0, 0.0, 5.0])],
            6.0,
        ))
        .unwrap();
    assert_eq!(part.feature_index(&fid), Some(1));
    part.set_eop(3).unwrap();
    assert!(part.feature_health.is_empty(), "{:?}", part.feature_health);
    let v = volume(&part);
    assert!(
        v < shelled - 1.0,
        "the round removes material: {v} vs {shelled}"
    );
    assert_cold_same(&part);
}

/// Reorder: allowed (two independent cuts) and refused (a pattern above
/// its source, the box below what is cut from it).
#[test]
fn reorder_allowed_and_refused() {
    let mut part = plate();
    let a = circle_feature(&mut part, (10.0, 10.0), 3.0, true, 1.0);
    let b = circle_feature(&mut part, (40.0, 20.0), 4.0, true, 1.0);
    let v = volume(&part);
    part.move_feature(2, 1).unwrap();
    assert_eq!(part.feature_index(&b), Some(1));
    assert_eq!(part.feature_index(&a), Some(2));
    assert!(close(volume(&part), v));
    assert!(part.feature_health.is_empty());
    // Sketches went along: each before its feature.
    for k in 1..3 {
        let s = part.features[k].sketch().unwrap();
        let ps = part.sketches.iter().find(|p| p.id == s).unwrap();
        assert!(ps.after_features.unwrap() <= k);
    }
    assert_cold_same(&part);
    // A pattern of `a` cannot go above `a`.
    part.add_pattern(pattern_of(&a, 3, 15.0)).unwrap();
    let before = (part.features.clone(), volume(&part));
    let err = part.move_feature(3, 1).unwrap_err().to_string();
    assert!(err.contains("uses"), "{err}");
    assert_eq!(part.features, before.0);
    assert!(close(volume(&part), before.1));
    // Nor can the box go below what is cut from it (the cuts' sketches lie
    // on its top face).
    let err = part.move_feature(0, 3).unwrap_err().to_string();
    assert!(err.contains("Box1"), "{err}");
    assert_eq!(part.features, before.0);
    // Across the End of Part: refused, move the marker instead.
    part.set_eop(3).unwrap();
    assert!(part.move_feature(1, 3).is_err());
}

/// A reorder that the references allow but whose rebuild fails is undone.
#[test]
fn reorder_that_fails_regeneration_is_rolled_back() {
    let mut q = PartDocument::default();
    let i = q
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &OPTS)
        .unwrap();
    q.sketches[i]
        .sketch
        .add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(10.0, 10.0))
        .unwrap();
    let rg = q.sketches[i].sketch.regions();
    let mut ex = ExtrudeFeature::new(vec![ProfileRef::of(&rg[0], rg[0].interior_point())], 5.0);
    ex.operation = Operation::NewSolid;
    q.add_extrude(i, ex).unwrap();
    let i2 = q
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &OPTS)
        .unwrap();
    q.sketches[i2]
        .sketch
        .add_rectangle_two_point(Point2::new(2.0, 2.0), Point2::new(4.0, 4.0))
        .unwrap();
    let rg = q.sketches[i2].sketch.regions();
    let mut cut = ExtrudeFeature::new(vec![ProfileRef::of(&rg[0], rg[0].interior_point())], 5.0);
    cut.operation = Operation::Cut;
    q.add_extrude(i2, cut).unwrap();
    let feats = q.features.clone();
    let places: Vec<_> = q.sketches.iter().map(|s| s.after_features).collect();
    let v = volume(&q);
    // The cut above the solid it cuts: nothing to cut, the rebuild fails
    // and the move is undone.
    let err = q.move_feature(1, 0).unwrap_err().to_string();
    assert!(err.contains("breaks the part"), "{err}");
    assert_eq!(q.features, feats);
    assert_eq!(
        q.sketches
            .iter()
            .map(|s| s.after_features)
            .collect::<Vec<_>>(),
        places
    );
    assert!(close(volume(&q), v));
    assert!(q.feature_health.is_empty());
}

/// Suppress a cut: its pattern goes with it; unsuppress brings both.
#[test]
fn suppress_and_unsuppress_with_dependants() {
    let mut part = plate();
    let a = circle_feature(&mut part, (10.0, 10.0), 3.0, true, 1.0);
    circle_feature(&mut part, (40.0, 30.0), 2.0, true, 1.0);
    let p = part.add_pattern(pattern_of(&a, 3, 15.0)).unwrap();
    let full = BOX - 3.0 * hole_v(3.0) - hole_v(2.0);
    assert!(close(volume(&part), full), "{}", volume(&part));
    let ids = part.suppress_feature(1).unwrap();
    assert_eq!(ids, vec![a.clone(), p.clone()]);
    assert!(part.is_suppressed(1) && part.is_suppressed(3) && !part.is_suppressed(2));
    assert!(close(volume(&part), BOX - hole_v(2.0)), "{}", volume(&part));
    assert!(part.feature_health.is_empty(), "{:?}", part.feature_health);
    assert_cold_same(&part);
    // Save / load keeps it.
    let json = serde_json::to_string(&part).unwrap();
    assert!(json.contains("\"suppressed\""));
    let mut q: PartDocument = serde_json::from_str(&json).unwrap();
    q.regenerate().unwrap();
    assert!(close(volume(&q), volume(&part)));
    let removed = part.unsuppress_feature(1).unwrap();
    assert_eq!(removed.len(), 2);
    assert!(part.suppressed.is_empty());
    assert!(close(volume(&part), full));
    assert_cold_same(&part);
    // Unsuppressing the pattern brings its source back too.
    part.suppress_feature(1).unwrap();
    part.unsuppress_feature(3).unwrap();
    assert!(part.suppressed.is_empty());
    // A part without a marker or suppressed features writes neither key.
    let json = serde_json::to_string(&part).unwrap();
    assert!(!json.contains("\"suppressed\"") && !json.contains("\"eop\""));
}

/// Delete with and without dependants and consumed sketches.
#[test]
fn delete_with_and_without_dependants() {
    let make = || {
        let mut part = plate();
        let a = circle_feature(&mut part, (10.0, 10.0), 3.0, true, 1.0);
        part.add_pattern(pattern_of(&a, 2, 20.0)).unwrap();
        // A free sketch on the top face.
        part.create_sketch(SketchPlacement::on_face("Box1:Top"), &OPTS)
            .unwrap();
        part
    };
    // Everything: the cut, its pattern, its sketch.
    let mut part = make();
    let plan = part.delete_plan(1, DeleteOptions::default());
    assert_eq!(plan.features, vec![1, 2]);
    assert_eq!(plan.sketches, vec![0]);
    part.delete_features(1, DeleteOptions::default()).unwrap();
    assert_eq!(part.features.len(), 1);
    assert_eq!(part.sketches.len(), 1, "the free sketch on the box stays");
    assert!(close(volume(&part), BOX));
    assert!(part.feature_health.is_empty());
    // Keep the dependants and the sketch: the pattern becomes sick, the
    // sketch becomes free and visible.
    let mut part = make();
    let keep = DeleteOptions {
        consumed_sketches: false,
        dependents: false,
    };
    let names = part.delete_features(1, keep).unwrap();
    assert_eq!(names, vec!["Extrusion1".to_owned()]);
    assert_eq!(part.features.len(), 2);
    assert_eq!(part.sketches.len(), 2);
    assert!(part.sketches[0].visible && !part.sketches[0].shared);
    let pid = part.features[1].id().to_owned();
    assert!(part.feature_health.contains_key(&pid), "sick, not silent");
    // Deleting the box takes the free sketch on its face too.
    let part = make();
    let plan = part.delete_plan(0, DeleteOptions::default());
    assert_eq!(plan.features, vec![0, 1, 2]);
    assert_eq!(plan.sketches, vec![0, 1]);
}

/// Rename keeps ids and every reference; names stay unique.
#[test]
fn rename_keeps_ids_and_references() {
    let mut part = plate();
    let a = circle_feature(&mut part, (10.0, 10.0), 3.0, true, 1.0);
    let v = volume(&part);
    part.rename_feature(1, "Pin hole").unwrap();
    assert_eq!(part.features[1].name(), "Pin hole");
    assert_eq!(part.features[1].id(), a);
    part.rename_feature(0, "Plate").unwrap();
    part.regenerate().unwrap();
    assert!(part.body.find_face("Box1:Top").is_some(), "names from ids");
    assert!(close(volume(&part), v));
    assert!(part.feature_health.is_empty());
    assert!(part.rename_feature(0, "Pin hole").is_err(), "taken");
    assert!(part.rename_feature(0, "  ").is_err());
    part.rename_sketch(0, "Holes").unwrap();
    assert_eq!(part.sketches[0].id, "Sketch1");
    assert_eq!(part.dependents(0), vec![1]);
}

/// A pattern of a feature inserted in the middle of the history, then the
/// later boss moved above it: volumes follow, the cache agrees with a cold
/// rebuild, and a save keeps the marker and the suppressed features.
#[test]
fn pattern_inserted_in_the_middle() {
    let mut part = plate();
    let a = circle_feature(&mut part, (10.0, 10.0), 2.0, true, 1.0);
    let boss = circle_feature(&mut part, (45.0, 30.0), 4.0, false, 5.0);
    part.set_eop(2).unwrap();
    let p = part.add_pattern(pattern_of(&a, 3, 12.0)).unwrap();
    assert_eq!(part.feature_index(&p), Some(2));
    assert!(close(volume(&part), BOX - 3.0 * hole_v(2.0)));
    part.set_eop(4).unwrap();
    let want = BOX - 3.0 * hole_v(2.0) + PI * 16.0 * 5.0;
    assert!(close(volume(&part), want), "{} vs {want}", volume(&part));
    assert_cold_same(&part);
    // The boss moved above the pattern: same body.
    part.move_feature(3, 2).unwrap();
    assert_eq!(part.feature_index(&boss), Some(2));
    assert!(close(volume(&part), want));
    assert_cold_same(&part);
    // Saved and loaded with a marker and a suppressed feature.
    part.set_eop(3).unwrap();
    part.suppress_feature(1).unwrap();
    let json = serde_json::to_string(&part).unwrap();
    let mut q: PartDocument = serde_json::from_str(&json).unwrap();
    q.regenerate().unwrap();
    assert_eq!(q.eop, Some(3));
    assert_eq!(q.suppressed, part.suppressed);
    assert!(close(volume(&q), volume(&part)));
    assert!(close(volume(&q), BOX + PI * 16.0 * 5.0), "{}", volume(&q));
}

/// Cache keys chain over the history: insertion, reorder and suppression
/// never reuse a stale step.
#[test]
fn cache_matches_cold_rebuild_after_each_edit() {
    let mut part = plate();
    circle_feature(&mut part, (10.0, 10.0), 3.0, true, 1.0);
    circle_feature(&mut part, (30.0, 20.0), 3.0, true, 1.0);
    part.add_blend(BlendFeature::fillet(
        vec![edge("Front", "Right", [60.0, 0.0, 5.0])],
        3.0,
    ))
    .unwrap();
    assert_cold_same(&part);
    part.set_eop(1).unwrap();
    assert_cold_same(&part);
    circle_feature(&mut part, (50.0, 30.0), 2.0, true, 1.0);
    assert_cold_same(&part);
    part.set_eop(5).unwrap();
    assert_cold_same(&part);
    part.move_feature(2, 3).unwrap();
    assert_cold_same(&part);
    part.suppress_feature(1).unwrap();
    assert_cold_same(&part);
    part.unsuppress_feature(1).unwrap();
    assert_cold_same(&part);
    part.delete_features(2, DeleteOptions::default()).unwrap();
    assert_cold_same(&part);
    assert!(part.feature_health.is_empty(), "{:?}", part.feature_health);
}

/// Delete All Features Below EOP.
#[test]
fn delete_below_eop() {
    let mut part = plate();
    circle_feature(&mut part, (10.0, 10.0), 3.0, true, 1.0);
    circle_feature(&mut part, (30.0, 20.0), 3.0, true, 1.0);
    part.set_eop(2).unwrap();
    part.create_sketch(SketchPlacement::on_face("Box1:Top"), &OPTS)
        .unwrap();
    let names = part.delete_below_eop().unwrap();
    assert_eq!(part.features.len(), 2);
    // Extrusion2 and its sketch; the sketch made at the marker stays.
    assert_eq!(names.len(), 2, "{names:?}");
    assert_eq!(part.sketches.len(), 2, "{names:?}");
    assert_eq!(part.eop, None);
}
