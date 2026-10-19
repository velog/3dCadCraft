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

/// Revision 3: a shell (top removed, 3 thick) and a fillet r2 on the outer
/// vertical edges after it.
fn shell_r3() -> PartDocument {
    let mut part = PartDocument::with_box("Box1", [60.0, 40.0, 20.0]).unwrap();
    let top = part.body.find_face("Box1:Top").unwrap();
    let mut def = cadcraft3d::part::ShellFeature::new(3.0);
    def.remove
        .push(cadcraft3d::part::shell::face_ref(&part.body, top, None).unwrap());
    part.add_shell(def).unwrap();
    let corner = |x: Real, w: Real| x.abs() < 1e-6 || (x - w).abs() < 1e-6;
    let vertical: Vec<EdgeRef> = part
        .body
        .edges()
        .filter_map(|(e, _)| edge_ref(&part.body, e))
        .filter(|r| corner(r.hint[0], 60.0) && corner(r.hint[1], 40.0))
        .filter(|r| r.hint[2] > 1e-6 && r.hint[2] < 20.0 - 1e-6)
        .collect();
    part.add_blend(BlendFeature::fillet(vertical, 2.0)).unwrap();
    part
}

/// Revision 4: Face Draft, fixed edges (front at its bottom, back at its
/// top), pull from the XY plane, 4°.
fn draft_r4() -> PartDocument {
    use cadcraft3d::part::{DraftFeature, PlaneRef, PullRef};
    let mut part = PartDocument::with_box("Box1", [60.0, 40.0, 20.0]).unwrap();
    let mut def = DraftFeature::new(4f64.to_radians());
    def.pull = Some(PullRef::Plane(PlaneRef::Origin(
        cadcraft3d::part::OriginPlane::XY,
    )));
    for (name, at) in [
        ("Box1:Front", [30.0, 0.0, 1.0]),
        ("Box1:Back", [30.0, 40.0, 19.0]),
    ] {
        let f = part.body.find_face(name).unwrap();
        def.faces.push(
            cadcraft3d::part::shell::face_ref(&part.body, f, Some(Point3::from(at))).unwrap(),
        );
    }
    part.add_draft(def).unwrap();
    part
}

/// Revision 5: Rectangular Pattern of a hole, Circular Pattern of a boss,
/// Mirror of the solid as a new solid.
fn pattern_r5() -> PartDocument {
    use cadcraft3d::part::{
        Extent, ExtentDirection, LineRef, Operation, PatternFeature, PatternKind, PlaneRef, PullRef,
    };
    let mut part = PartDocument::with_box("Box1", [60.0, 40.0, 10.0]).unwrap();
    let opts = SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    };
    // A hole r 3 at (10, 10) through, a boss r 2 × 3 at (30, 10) on top.
    for (at, r, cut) in [((10.0, 10.0), 3.0, true), ((30.0, 10.0), 2.0, false)] {
        let j = part
            .create_sketch(SketchPlacement::on_face("Box1:Top"), &opts)
            .unwrap();
        let c = part.sketches[j]
            .sketch
            .plane
            .project(&Point3::new(at.0, at.1, 10.0));
        part.sketches[j].sketch.add_circle(c, r).unwrap();
        let rg = part.sketches[j].sketch.regions();
        let mut ex = ExtrudeFeature::new(vec![ProfileRef::of(&rg[0], rg[0].interior_point())], 3.0);
        if cut {
            ex.operation = Operation::Cut;
            ex.extent = Extent::ThroughAll;
            ex.direction = ExtentDirection::Flipped;
        } else {
            ex.operation = Operation::Join;
        }
        part.add_extrude(j, ex).unwrap();
    }
    let mut rect = PatternFeature::rectangular();
    rect.features = vec!["Extrusion1".into()];
    if let PatternKind::Rectangular { first, .. } = &mut rect.kind {
        first.dir = Some(PullRef::Line(LineRef::Origin(0)));
        first.count = 3;
        first.spacing = 20.0;
    }
    part.add_pattern(rect).unwrap();
    let mut circ = PatternFeature::circular();
    circ.features = vec!["Extrusion2".into()];
    if let PatternKind::Circular {
        axis,
        count,
        angle,
        fitted,
        ..
    } = &mut circ.kind
    {
        (*axis, *count, *angle, *fitted) = (Some(LineRef::Origin(2)), 3, 15f64.to_radians(), false);
    }
    part.add_pattern(circ).unwrap();
    let mut m = PatternFeature::mirror();
    m.solid_mode = true;
    m.operation = Operation::NewSolid;
    m.kind = PatternKind::Mirror {
        plane: Some(PlaneRef::Origin(OriginPlane::YZ)),
    };
    part.add_pattern(m).unwrap();
    part
}
/// Revision 6: Hole (two counterbored M6 clearance holes, a tapped M8 hole)
/// and a cosmetic Thread in the tapped hole's wall.
fn hole_r6() -> PartDocument {
    use cadcraft3d::part::hole::HoleCenter;
    use cadcraft3d::part::standards::{self, Standards};
    use cadcraft3d::part::{FastenerFit, HoleFeature, HoleSpec, ThreadFeature, ThreadSpec};
    let t = Standards::builtin();
    let mut part = PartDocument::with_box("Box1", [60.0, 40.0, 20.0]).unwrap();
    let add = |part: &mut PartDocument, spec: HoleSpec, at: &[Point3]| {
        let (i, ids) = part.create_hole_sketch("Box1:Top", at).unwrap();
        let mut def = HoleFeature::new(spec);
        def.sketch = part.sketches[i].id.clone();
        def.centers = ids.into_iter().map(HoleCenter::Point).collect();
        part.add_hole(def).unwrap();
    };
    let mut cb = HoleSpec::clearance(&t, "ISO 273", "M6", FastenerFit::Normal).unwrap();
    assert!(cb.counterbore_from(&t, "Socket Head Cap Screw ISO 4762", "M6"));
    add(
        &mut part,
        cb,
        &[Point3::new(10.0, 10.0, 20.0), Point3::new(50.0, 10.0, 20.0)],
    );
    let m8 = ThreadSpec::from_table(&t, standards::ISO_METRIC, "M8", true).unwrap();
    add(
        &mut part,
        HoleSpec::tapped(m8.clone(), 15.0),
        &[Point3::new(30.0, 25.0, 20.0)],
    );
    let wall = part.body.find_face("Hole2:Bore:1").unwrap();
    let face =
        cadcraft3d::part::shell::face_ref(&part.body, wall, Some(Point3::new(30.0, 28.3, 19.0)))
            .unwrap();
    part.add_thread(ThreadFeature::new(face, m8))
        .map_err(|e| e.to_string())
        .unwrap();
    part
}

/// Revision 7: End of Part and suppressed features. Box, two through
/// holes, a pattern of the first; the second hole suppressed, the marker
/// above the pattern (built: the box and the first hole).
fn history_r7() -> PartDocument {
    use cadcraft3d::part::{
        Extent, ExtentDirection, LineRef, Operation, PatternFeature, PatternKind, PullRef,
    };
    let mut part = PartDocument::with_box("Box1", [60.0, 40.0, 10.0]).unwrap();
    let opts = SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    };
    for (x, y) in [(10.0, 10.0), (40.0, 30.0)] {
        let j = part
            .create_sketch(SketchPlacement::on_face("Box1:Top"), &opts)
            .unwrap();
        let c = part.sketches[j]
            .sketch
            .plane
            .project(&Point3::new(x, y, 10.0));
        part.sketches[j].sketch.add_circle(c, 3.0).unwrap();
        let rg = part.sketches[j].sketch.regions();
        let mut ex = ExtrudeFeature::new(vec![ProfileRef::of(&rg[0], rg[0].interior_point())], 1.0);
        ex.operation = Operation::Cut;
        ex.extent = Extent::ThroughAll;
        ex.direction = ExtentDirection::Flipped;
        part.add_extrude(j, ex).unwrap();
    }
    let mut rect = PatternFeature::rectangular();
    rect.features = vec!["Extrusion1".into()];
    if let PatternKind::Rectangular { first, .. } = &mut rect.kind {
        first.dir = Some(PullRef::Line(LineRef::Origin(0)));
        first.count = 3;
        first.spacing = 15.0;
    }
    part.add_pattern(rect).unwrap();
    part.suppress_feature(2).unwrap();
    part.set_eop(3).unwrap();
    part
}
/// Revision 8: work features on curved faces and a UCS: a cylinder, a
/// tangent work plane parallel to YZ, an axis through the cylinder face, a
/// UCS lifted to the top with a boss sketched on its XY plane.
fn work_r8() -> PartDocument {
    use cadcraft3d::kernel::Surface;
    use cadcraft3d::part::{UcsDef, WorkAxisDef, WorkDef, WorkFeature, WorkPlaneDef};
    let mut part = PartDocument::default();
    let o = SketchOptions {
        auto_project_origin: false,
        ..Default::default()
    };
    let i = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &o)
        .unwrap();
    let sk = &mut part.sketches[i].sketch;
    sk.add_circle(Point2::new(0.0, 0.0), 10.0).unwrap();
    let r = sk.regions();
    part.add_extrude(
        i,
        ExtrudeFeature::new(vec![ProfileRef::of(&r[0], r[0].interior_point())], 20.0),
    )
    .unwrap();
    let face = part
        .body
        .faces()
        .find(|(_, f)| matches!(part.body.surface(f.surface), Some(Surface::Cylinder(_))))
        .and_then(|(_, f)| f.name.clone())
        .unwrap();
    part.add_work(WorkFeature::new(WorkDef::Plane(
        WorkPlaneDef::TangentParallel {
            face: face.clone(),
            base: cadcraft3d::part::PlaneRef::Origin(OriginPlane::YZ),
            other: false,
        },
    )))
    .unwrap();
    part.add_work(WorkFeature::new(WorkDef::Axis(WorkAxisDef::RevolvedFace {
        face,
    })))
    .unwrap();
    part.add_work(WorkFeature::new(WorkDef::Ucs(UcsDef::Offsets {
        x: 0.0,
        y: 0.0,
        z: 20.0,
        rx: 0.0,
        ry: 0.0,
        rz: 0.3,
    })))
    .unwrap();
    let j = part
        .create_sketch(
            SketchPlacement {
                plane: cadcraft3d::part::PlaneRef::Work("UCS1:XY".into()),
                ..Default::default()
            },
            &o,
        )
        .unwrap();
    let sk = &mut part.sketches[j].sketch;
    sk.add_rectangle_two_point(Point2::new(-3.0, -2.0), Point2::new(3.0, 2.0))
        .unwrap();
    let r = sk.regions();
    let mut ex = ExtrudeFeature::new(vec![ProfileRef::of(&r[0], r[0].interior_point())], 5.0);
    ex.operation = cadcraft3d::part::Operation::Join;
    part.add_extrude(j, ex).unwrap();
    part
}

/// Revision 9: Face Draft Symmetric about a work plane, Parting Line from
/// a sketch (Asymmetric), Shell Center with Rounded Corners.
fn draft_r9() -> PartDocument {
    use cadcraft3d::part::{
        DraftExtent, DraftFeature, DraftType, PartingTool, PlaneRef, PullRef, ShellCorners,
        ShellDirection, ShellFeature, WorkDef, WorkFeature, WorkPlaneDef,
    };
    let mut part = PartDocument::with_box("Box1", [60.0, 40.0, 20.0]).unwrap();
    let wp = part
        .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
            base: PlaneRef::Origin(OriginPlane::XY),
            distance: 10.0,
        })))
        .unwrap();
    let face = |part: &PartDocument, name: &str, at: [f64; 3]| {
        let f = part.body.find_face(name).unwrap();
        cadcraft3d::part::shell::face_ref(&part.body, f, Some(Point3::from(at))).unwrap()
    };
    let mut d = DraftFeature::new(3f64.to_radians());
    d.draft_type = DraftType::FixedPlane;
    d.plane = Some(PlaneRef::Work(wp));
    d.extent = DraftExtent::Symmetric;
    d.faces.push(face(&part, "Box1:Front", [30.0, 0.0, 15.0]));
    d.faces.push(face(&part, "Box1:Back", [30.0, 40.0, 15.0]));
    part.add_draft(d).unwrap();
    let j = part
        .create_sketch(
            SketchPlacement::on_origin_plane(OriginPlane::YZ),
            &SketchOptions::default(),
        )
        .unwrap();
    {
        let sk = &mut part.sketches[j].sketch;
        let a = sk.add_point(Point2::new(-5.0, 6.0));
        let b = sk.add_point(Point2::new(45.0, 6.0));
        sk.add_line_between(a, b).unwrap();
    }
    let mut d = DraftFeature::new(2f64.to_radians());
    d.set_type(DraftType::PartingLine);
    d.pull = Some(PullRef::Plane(PlaneRef::Origin(OriginPlane::XY)));
    d.parting = Some(PartingTool::Sketch(part.sketches[j].id.clone()));
    d.extent = DraftExtent::Asymmetric;
    d.angle2 = 4f64.to_radians();
    d.faces.push(face(&part, "Box1:Left", [0.0, 20.0, 10.0]));
    d.faces.push(face(&part, "Box1:Right", [60.0, 20.0, 10.0]));
    part.add_draft(d).unwrap();
    let mut s = ShellFeature::new(2.0);
    s.direction = ShellDirection::Center;
    s.corners = ShellCorners::Rounded;
    s.remove.push(face(&part, "Box1:Top", [30.0, 20.0, 20.0]));
    part.add_shell(s).unwrap();
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
        let f = root().join("r4/draft_r4.3dcadcraft");
        if !f.exists() {
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            let mut project = Project::new("DraftR4");
            project.add_document("Part1", DocumentData::Part(draft_r4()));
            project.save(&f).unwrap();
        }
        let f = root().join("r5/pattern_r5.3dcadcraft");
        if !f.exists() {
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            let mut project = Project::new("PatternR5");
            project.add_document("Part1", DocumentData::Part(pattern_r5()));
            project.save(&f).unwrap();
        }
        let f = root().join("r6/hole_r6.3dcadcraft");
        if !f.exists() {
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            let mut project = Project::new("HoleR6");
            project.add_document("Part1", DocumentData::Part(hole_r6()));
            project.save(&f).unwrap();
        }
        let f = root().join("r7/history_r7.3dcadcraft");
        if !f.exists() {
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            let mut project = Project::new("HistoryR7");
            project.add_document("Part1", DocumentData::Part(history_r7()));
            project.save(&f).unwrap();
        }
        let f = root().join("r8/work_r8.3dcadcraft");
        if !f.exists() {
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            let mut project = Project::new("WorkR8");
            project.add_document("Part1", DocumentData::Part(work_r8()));
            project.save(&f).unwrap();
        }
        let f = root().join("r9/draft_r9.3dcadcraft");
        if !f.exists() {
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            let mut project = Project::new("DraftR9");
            project.add_document("Part1", DocumentData::Part(draft_r9()));
            project.save(&f).unwrap();
        }
        let f = root().join("r3/shell_r3.3dcadcraft");
        if !f.exists() {
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            let mut project = Project::new("ShellR3");
            project.add_document("Part1", DocumentData::Part(shell_r3()));
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
