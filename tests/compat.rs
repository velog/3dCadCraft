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

/// Revision 10: a hole with an Angle Y drill point at a Center Point
/// dimensioned to a projected box edge, and a Flat W hole.
fn hole_r10() -> PartDocument {
    use cadcraft3d::part::hole::HoleCenter;
    use cadcraft3d::part::{DrillPoint, HoleFeature, HoleSpec};
    let mut part = PartDocument::with_box("Box1", [60.0, 40.0, 20.0]).unwrap();
    let (i, ids) = part
        .create_hole_sketch("Box1:Top", &[Point3::new(15.0, 12.0, 20.0)])
        .unwrap();
    let line = part.project_segment(i, "Box1:V001", "Box1:V101").unwrap();
    part.sketches[i]
        .sketch
        .add_constraint(cadcraft3d::sketch::Constraint::PointLineDistance {
            point: ids[0],
            line,
            value: 12.0,
        })
        .unwrap();
    let mut s = HoleSpec::simple(8.0, 12.0);
    s.drill_point = DrillPoint::AngleY;
    let mut def = HoleFeature::new(s);
    def.sketch = part.sketches[i].id.clone();
    def.centers = ids.into_iter().map(HoleCenter::Point).collect();
    part.add_hole(def).unwrap();
    let (j, ids) = part
        .create_hole_sketch("Box1:Top", &[Point3::new(45.0, 25.0, 20.0)])
        .unwrap();
    let mut s = HoleSpec::simple(5.0, 8.0);
    s.drill_point = DrillPoint::FlatW;
    let mut def = HoleFeature::new(s);
    def.sketch = part.sketches[j].id.clone();
    def.centers = ids.into_iter().map(HoleCenter::Point).collect();
    part.add_hole(def).unwrap();
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
/// Revision 12: a Sweep (a circle along an arc on XZ) and a Loft (a square
/// to a smaller square) joined to it.
/// Revision 14: a Coil (closed ends, left hand) beside a plate with a Web
/// across it (To Next) and a boss.
/// Revision 18: a true / false and a text parameter, a size chosen by
/// `if()` on them, and a boss suppressed by a condition (Suppress if).
fn logic_r18() -> PartDocument {
    use cadcraft3d::part::expr::Unit;
    use cadcraft3d::part::{Operation, ProfileRef};
    let mut part = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
    let heavy = part.add_user_parameter(Unit::Bool);
    part.set_parameter_expr(&heavy, "false").unwrap();
    let mat = part.add_user_parameter(Unit::Text);
    part.set_parameter_expr(&mat, "\"Al\"").unwrap();
    let w = part.add_user_parameter(Unit::Mm);
    part.set_parameter_expr(
        &w,
        &format!("if({heavy} or {mat} == \"Steel\", 40 mm, 25 mm)"),
    )
    .unwrap();
    part.set_feature_field(0, "width", &w).unwrap();
    let o = SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    };
    let j = part
        .create_sketch(SketchPlacement::on_face("Box1:Top"), &o)
        .unwrap();
    let c = part.sketches[j]
        .sketch
        .plane
        .project(&Point3::new(20.0, 12.0, 20.0));
    part.sketches[j].sketch.add_circle(c, 5.0).unwrap();
    let rg = part.sketches[j].sketch.regions();
    let mut ex = ExtrudeFeature::new(vec![ProfileRef::of(&rg[0], rg[0].interior_point())], 10.0);
    ex.operation = Operation::Join;
    part.add_extrude(j, ex).unwrap();
    let k = part.last_inserted();
    part.set_suppress_if(k, Some(&format!("not {heavy}")))
        .unwrap();
    part.regenerate().unwrap();
    part
}
/// Revision 19: a sheet metal part — a 100 × 60 Face, 2 mm (the default
/// rule), with a 25 mm Flange on its front edge.
fn sheet_r19() -> PartDocument {
    use cadcraft3d::part::sheet_metal::SheetFlangeFeature;
    use cadcraft3d::part::{OriginPlane, ProfileRef};
    use cadcraft3d::sketch::Point2;
    let mut p = PartDocument::default();
    p.make_sheet_metal(None);
    let o = SketchOptions {
        auto_project_origin: false,
        ..Default::default()
    };
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &o)
        .unwrap();
    p.sketches[i]
        .sketch
        .add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(100.0, 60.0))
        .unwrap();
    let r = p.sketches[i].sketch.regions();
    let prof = vec![ProfileRef::of(&r[0], r[0].interior_point())];
    p.add_sheet_face(i, prof, false).unwrap();
    let (body, edge) = p.sheet_edge_at(Point3::new(50.0, 0.0, 2.0)).unwrap();
    p.add_sheet_flange(SheetFlangeFeature::new(&body, vec![edge], 25.0))
        .unwrap();
    p
}

/// Revision 22: a 120 × 80 tray (four flanges) whose front-right corner is an
/// Overlap Corner Seam.
fn sheet_r22() -> PartDocument {
    use cadcraft3d::part::sheet_metal::{SeamType, SheetFlangeFeature, SheetOp, SheetOpFeature};
    use cadcraft3d::part::{OriginPlane, ProfileRef};
    use cadcraft3d::sketch::Point2;
    let mut p = PartDocument::default();
    p.make_sheet_metal(None);
    let o = SketchOptions {
        auto_project_origin: false,
        ..Default::default()
    };
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &o)
        .unwrap();
    p.sketches[i]
        .sketch
        .add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(120.0, 80.0))
        .unwrap();
    let r = p.sketches[i].sketch.regions();
    p.add_sheet_face(i, vec![ProfileRef::of(&r[0], r[0].interior_point())], false)
        .unwrap();
    let mut edges = Vec::new();
    let mut body = String::new();
    for at in [(60.0, 0.0), (120.0, 40.0), (60.0, 80.0), (0.0, 40.0)] {
        let (b, e) = p.sheet_edge_at(Point3::new(at.0, at.1, 2.0)).unwrap();
        body = b;
        edges.push(e);
    }
    p.add_sheet_flange(SheetFlangeFeature::new(&body, edges, 20.0))
        .unwrap();
    let c = p
        .sheet_corner_near(Point3::new(120.0, 0.0, 15.0), 100.0)
        .unwrap();
    p.add_sheet_op(SheetOpFeature::new(SheetOp::CornerSeam {
        body: c.body,
        a: c.a,
        b: c.b,
        seam: SeamType::Overlap,
        gap: None,
        ratio: 1.0,
    }))
    .unwrap();
    p
}

/// Revision 21: a 100 × 60 Face with a Hem on its right edge, a Cut (a
/// circle) and a Fold along x = 70.
fn sheet_r21() -> PartDocument {
    use cadcraft3d::part::sheet_metal::{FoldLocation, SheetOp, SheetOpFeature};
    use cadcraft3d::part::{OriginPlane, ProfileRef};
    use cadcraft3d::sketch::Point2;
    let mut p = PartDocument::default();
    p.make_sheet_metal(None);
    let o = SketchOptions {
        auto_project_origin: false,
        ..Default::default()
    };
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &o)
        .unwrap();
    p.sketches[i]
        .sketch
        .add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(100.0, 60.0))
        .unwrap();
    let r = p.sketches[i].sketch.regions();
    let prof = vec![ProfileRef::of(&r[0], r[0].interior_point())];
    p.add_sheet_face(i, prof, false).unwrap();
    let (body, edge) = p.sheet_edge_at(Point3::new(100.0, 30.0, 2.0)).unwrap();
    p.add_sheet_op(SheetOpFeature::new(SheetOp::Hem {
        body: body.clone(),
        edges: vec![edge],
        length: 8.0,
        gap: None,
        flip: false,
    }))
    .unwrap();
    let j = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &o)
        .unwrap();
    p.sketches[j]
        .sketch
        .add_circle(Point2::new(30.0, 30.0), 6.0)
        .unwrap();
    let line = p.sketches[j]
        .sketch
        .add_line(Point2::new(70.0, -5.0), Point2::new(70.0, 65.0));
    let r = p.sketches[j].sketch.regions();
    let circle = r
        .iter()
        .min_by(|a, b| a.area().total_cmp(&b.area()))
        .unwrap();
    let sid = p.sketches[j].id.clone();
    p.add_sheet_op(SheetOpFeature::new(SheetOp::Cut {
        extent: Default::default(),
        depth: 0.0,
        across: false,
        sketch: sid.clone(),
        profiles: vec![ProfileRef::of(circle, circle.interior_point())],
    }))
    .unwrap();
    p.add_sheet_op(SheetOpFeature::new(SheetOp::Fold {
        body,
        sketch: sid,
        line: cadcraft3d::part::extrude::entity_key(line),
        angle: std::f64::consts::FRAC_PI_2,
        radius: None,
        location: FoldLocation::Centerline,
        flip: false,
        flip_side: false,
    }))
    .unwrap();
    p
}

/// Revision 20: a 100 × 60 × 40 box converted to sheet metal (Top fixed,
/// its front and back edges bent), Square corner reliefs, and a Flange
/// 20 wide centred on the front wall's foot.
fn sheet_r20() -> PartDocument {
    use cadcraft3d::part::EdgeRef;
    use cadcraft3d::part::sheet_metal::{
        CornerRelief, FlangeWidth, SheetConvertFeature, SheetFlangeFeature, SheetMetalDefaults,
    };
    use cadcraft3d::part::shell::FaceRef;
    let mut p = PartDocument::with_box("Box1", [100.0, 60.0, 40.0]).unwrap();
    p.make_sheet_metal(Some(SheetMetalDefaults {
        corner_shape: CornerRelief::Square,
        ..Default::default()
    }));
    let e = |a: &str, b: &str, hint: [f64; 3]| EdgeRef {
        faces: [a.into(), b.into()],
        hint,
    };
    p.add_sheet_convert(SheetConvertFeature {
        id: String::new(),
        name: String::new(),
        body: "Box1".into(),
        face: FaceRef {
            face: "Box1:Top".into(),
            hint: [50.0, 30.0, 40.0],
        },
        bends: vec![
            e("Box1:Top", "Box1:Front", [50.0, 0.0, 40.0]),
            e("Box1:Top", "Box1:Back", [50.0, 60.0, 40.0]),
        ],
    })
    .unwrap();
    let (body, edge) = p.sheet_edge_at(Point3::new(50.0, 1.0, 0.0)).unwrap();
    let mut fl = SheetFlangeFeature::new(&body, vec![edge], 15.0);
    fl.extents = FlangeWidth::Width;
    fl.width = 20.0;
    fl.centered = true;
    p.add_sheet_flange(fl).unwrap();
    p
}

/// Revision 17: user parameters in cm, in and rad (`Unit::Cm` / `In` / `Rad`)
/// driving the box (length 6 cm, width 1 in + 5 mm).
fn units_r17() -> PartDocument {
    use cadcraft3d::part::expr::Unit;
    let mut part = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
    let a = part.add_user_parameter(Unit::Cm);
    part.set_parameter_expr(&a, "6").unwrap();
    let b = part.add_user_parameter(Unit::In);
    part.set_parameter_expr(&b, "1 + 5 mm").unwrap();
    let c = part.add_user_parameter(Unit::Rad);
    part.set_parameter_expr(&c, "PI / 4").unwrap();
    part.set_feature_field(0, "length", &a).unwrap();
    part.set_feature_field(0, "width", &b).unwrap();
    part.regenerate().unwrap();
    part
}
/// Revision 16: a work point and a work axis projected into a sketch
/// (`LinkSource::Work`), a boss extruded from a circle on the point.
fn project_work_r16() -> PartDocument {
    use cadcraft3d::part::{WorkAxisDef, WorkDef, WorkFeature, WorkPointDef};
    let o = SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    };
    let mut part = PartDocument::default();
    let pt = part
        .add_work(WorkFeature::new(WorkDef::Point(WorkPointDef::Fixed {
            x: 10.0,
            y: 5.0,
            z: 0.0,
        })))
        .unwrap();
    let ax = part
        .add_work(WorkFeature::new(WorkDef::Axis(WorkAxisDef::Fixed {
            origin: [0.0, -5.0, 0.0],
            dir: [1.0, 0.0, 0.0],
        })))
        .unwrap();
    let i = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &o)
        .unwrap();
    let l = part.project_work(i, &pt).unwrap();
    part.project_work(i, &ax).unwrap();
    let c = part.sketches[i].links[l].points[0];
    let sk = &mut part.sketches[i].sketch;
    sk.add_circle_at(c, 3.0).unwrap();
    let r = sk.regions();
    let pr = ProfileRef::of(&r[0], r[0].interior_point());
    part.add_extrude(i, ExtrudeFeature::new(vec![pr], 4.0))
        .unwrap();
    part
}

/// Revision 15: a ring revolved around a work axis (`AxisRef::Work`).
fn revolve_work_r15() -> PartDocument {
    use cadcraft3d::part::{AxisRef, RevolveFeature, WorkAxisDef, WorkDef, WorkFeature};
    let o = SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    };
    let mut part = PartDocument::default();
    part.add_work(WorkFeature::new(WorkDef::Axis(WorkAxisDef::Fixed {
        origin: [5.0, 0.0, 0.0],
        dir: [0.0, 0.0, 1.0],
    })))
    .unwrap();
    let axis = part.features[0].id().to_owned();
    let i = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XZ), &o)
        .unwrap();
    let sk = &mut part.sketches[i].sketch;
    sk.add_rectangle_two_point(Point2::new(10.0, 0.0), Point2::new(20.0, 5.0))
        .unwrap();
    let r = sk.regions();
    let pr = ProfileRef::of(&r[0], r[0].interior_point());
    part.add_revolve(i, RevolveFeature::new(vec![pr], AxisRef::Work(axis)))
        .unwrap();
    part
}

fn coil_rib_r14() -> PartDocument {
    use cadcraft3d::part::work::LineRef;
    use cadcraft3d::part::{
        CoilAxis, CoilFeature, Operation, PlaneRef, RibFeature, WorkDef, WorkFeature, WorkPlaneDef,
    };
    let o = SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    };
    let mut part = PartDocument::default();
    // A plate 40 × 30 × 4 with a 2 mm rim 6 high (an open tray).
    let k = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &o)
        .unwrap();
    part.sketches[k]
        .sketch
        .add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(40.0, 30.0))
        .unwrap();
    let r = part.sketches[k].sketch.regions();
    let pr = vec![ProfileRef::of(&r[0], r[0].interior_point())];
    part.add_extrude(k, ExtrudeFeature::new(pr, 10.0)).unwrap();
    let wp = part
        .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
            base: PlaneRef::Origin(OriginPlane::XY),
            distance: 10.0,
        })))
        .unwrap();
    let on = SketchPlacement {
        plane: PlaneRef::Work(wp),
        ..Default::default()
    };
    let k = part.create_sketch(on.clone(), &o).unwrap();
    part.sketches[k]
        .sketch
        .add_rectangle_two_point(Point2::new(2.0, 2.0), Point2::new(38.0, 28.0))
        .unwrap();
    let r = part.sketches[k].sketch.regions();
    let pr = vec![ProfileRef::of(&r[0], r[0].interior_point())];
    let mut cut = ExtrudeFeature::new(pr, 6.0);
    cut.operation = Operation::Cut;
    cut.direction = cadcraft3d::part::ExtentDirection::Flipped;
    part.add_extrude(k, cut).unwrap();
    let k = part.create_sketch(on, &o).unwrap();
    let l = part.sketches[k]
        .sketch
        .add_line(Point2::new(2.0, 15.0), Point2::new(38.0, 15.0));
    let c = part.sketches[k].sketch.add_point(Point2::new(20.0, 15.0));
    let mut web = RibFeature::new(vec![l], 2.0);
    web.boss.centers = vec![c];
    part.add_rib(k, web).unwrap();
    // A spring beside it.
    let k = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XZ), &o)
        .unwrap();
    part.sketches[k]
        .sketch
        .add_circle(Point2::new(-20.0, 1.0), 1.0)
        .unwrap();
    let r = part.sketches[k].sketch.regions();
    let pr = vec![ProfileRef::of(&r[0], r[0].interior_point())];
    let mut coil = CoilFeature::new(pr, CoilAxis::Line(LineRef::Origin(2)));
    coil.axis = CoilAxis::Line(LineRef::Origin(2));
    (coil.pitch, coil.revolutions, coil.left_hand) = (5.0, 2.5, true);
    coil.start.closed = true;
    coil.end.closed = true;
    part.add_coil(k, coil).unwrap();
    part
}

fn sweep_loft_r12() -> PartDocument {
    use cadcraft3d::part::{
        LoftFeature, LoftSection, Operation, PathRef, PlaneRef, SweepFeature, WorkDef, WorkFeature,
        WorkPlaneDef,
    };
    let mut part = PartDocument::default();
    let o = SketchOptions {
        auto_project_origin: false,
        ..Default::default()
    };
    let a = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &o)
        .unwrap();
    part.sketches[a]
        .sketch
        .add_circle(Point2::new(0.0, 0.0), 4.0)
        .unwrap();
    let b = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XZ), &o)
        .unwrap();
    {
        let s = &mut part.sketches[b].sketch;
        let c = s.add_point(Point2::new(30.0, 0.0));
        let t = s.add_point(Point2::new(30.0, 30.0));
        let z = s.add_point(Point2::new(0.0, 0.0));
        s.add_arc(c, t, z).unwrap();
    }
    let r = part.sketches[a].sketch.regions();
    let path = PathRef::Sketch {
        sketch: part.sketches[b].id.clone(),
        curves: part.sketches[b]
            .sketch
            .entities()
            .map(|(id, _)| id)
            .collect(),
    };
    let def = SweepFeature::new(vec![ProfileRef::of(&r[0], r[0].interior_point())], path);
    part.add_sweep(a, def).unwrap();
    let wp = part
        .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
            base: PlaneRef::Origin(OriginPlane::XY),
            distance: -20.0,
        })))
        .unwrap();
    let c = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &o)
        .unwrap();
    part.sketches[c]
        .sketch
        .add_rectangle_two_point(Point2::new(-10.0, -10.0), Point2::new(10.0, 10.0))
        .unwrap();
    let d = part
        .create_sketch(
            SketchPlacement {
                plane: PlaneRef::Work(wp),
                ..Default::default()
            },
            &o,
        )
        .unwrap();
    part.sketches[d]
        .sketch
        .add_rectangle_two_point(Point2::new(-5.0, -5.0), Point2::new(5.0, 5.0))
        .unwrap();
    let sec = |part: &PartDocument, k: usize| {
        let r = part.sketches[k].sketch.regions();
        LoftSection::Profile {
            sketch: part.sketches[k].id.clone(),
            profile: ProfileRef::of(&r[0], r[0].interior_point()),
        }
    };
    let mut loft = LoftFeature::new(vec![sec(&part, c), sec(&part, d)]);
    loft.operation = Operation::Join;
    part.add_loft(loft).unwrap();
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

/// Revision 11: Extrude / Revolve options from Inventor 2027 — primitives,
/// To an inclined work plane, an open profile (Match Shape), Revolve To a
/// plane through the axis with Minimum Solution.
/// Revision 13: Combine (Join, Keep Toolbody: the tool stays hidden) and
/// Split (Split Face of the top, Split Solid by an origin plane).
fn combine_split_r13() -> PartDocument {
    use cadcraft3d::part::{CombineFeature, PlaneRef, SplitFeature, SplitMethod, SplitToolRef};
    let mut part = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
    let i = part
        .create_sketch(
            cadcraft3d::part::SketchPlacement::on_origin_plane(OriginPlane::XY),
            &cadcraft3d::part::SketchOptions {
                auto_project_origin: false,
                ..Default::default()
            },
        )
        .unwrap();
    let sk = &mut part.sketches[i].sketch;
    sk.add_rectangle_two_point(
        cadcraft3d::sketch::Point2::new(30.0, 10.0),
        cadcraft3d::sketch::Point2::new(60.0, 20.0),
    )
    .unwrap();
    let r = sk.regions();
    let pr = vec![cadcraft3d::part::ProfileRef::of(
        &r[0],
        r[0].interior_point(),
    )];
    let tool = part
        .add_extrude(i, cadcraft3d::part::ExtrudeFeature::new(pr, 10.0))
        .unwrap();
    let mut c = CombineFeature::new();
    c.base = "Box1".into();
    c.tools = vec![tool];
    c.keep_tools = true;
    part.add_combine(c).unwrap();
    let mut s = SplitFeature::new(SplitMethod::SplitFace);
    s.all_faces = true;
    s.solid = "Box1".into();
    let wp = part
        .add_work(cadcraft3d::part::WorkFeature::new(
            cadcraft3d::part::WorkDef::Plane(cadcraft3d::part::WorkPlaneDef::Offset {
                base: PlaneRef::Origin(OriginPlane::YZ),
                distance: 20.0,
            }),
        ))
        .unwrap();
    s.tool = Some(SplitToolRef::Plane(PlaneRef::Work(wp)));
    part.add_split(s).unwrap();
    let mut s = SplitFeature::new(SplitMethod::SplitSolid);
    let wp = part
        .add_work(cadcraft3d::part::WorkFeature::new(
            cadcraft3d::part::WorkDef::Plane(cadcraft3d::part::WorkPlaneDef::Offset {
                base: PlaneRef::Origin(OriginPlane::XY),
                distance: 5.0,
            }),
        ))
        .unwrap();
    s.tool = Some(SplitToolRef::Plane(PlaneRef::Work(wp)));
    s.solid = "Box1".into();
    part.add_split(s).unwrap();
    part
}

fn extrude_options_r11() -> PartDocument {
    use cadcraft3d::part::primitive::add_primitive;
    use cadcraft3d::part::{
        AxisRef, Extent, LineRef, OpenProfile, Operation, PlaneRef, PrimitiveFeature,
        PrimitiveKind, RevolveExtent, RevolveFeature, TargetRef, WorkDef, WorkFeature,
        WorkPlaneDef,
    };
    let opts = SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    };
    let xy = || SketchPlacement::on_origin_plane(OriginPlane::XY);
    let mut part = PartDocument::default();
    let f = PrimitiveFeature::extrude(&part, 40.0);
    add_primitive(
        &mut part,
        PrimitiveKind::Box,
        xy(),
        &[Point2::new(40.0, 30.0), Point2::new(80.0, 60.0)],
        f,
    )
    .unwrap();
    let tilted = part
        .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Angle {
            base: PlaneRef::Origin(OriginPlane::XY),
            axis: LineRef::Origin(1),
            angle: 0.3,
        })))
        .unwrap();
    let top = part
        .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
            base: PlaneRef::Origin(OriginPlane::XY),
            distance: 40.0,
        })))
        .unwrap();
    let mut e = ExtrudeFeature::new(Vec::new(), 1.0);
    e.extent = Extent::To(TargetRef::Plane(PlaneRef::Work(tilted)));
    e.operation = Operation::Cut;
    add_primitive(
        &mut part,
        PrimitiveKind::Cylinder,
        SketchPlacement {
            plane: PlaneRef::Work(top),
            ..Default::default()
        },
        &[Point2::new(40.0, 30.0), Point2::new(48.0, 30.0)],
        PrimitiveFeature::Extrude(e),
    )
    .unwrap();
    // An open line across the block: cut one side off, through all.
    let i = part.create_sketch(xy(), &opts).unwrap();
    let l = part.sketches[i]
        .sketch
        .add_line(Point2::new(70.0, -5.0), Point2::new(70.0, 65.0));
    let mut cut = ExtrudeFeature::new(Vec::new(), 1.0);
    cut.open = Some(OpenProfile {
        curves: vec![l],
        flip: true,
        match_shape: true,
    });
    cut.extent = Extent::ThroughAll;
    cut.operation = Operation::Cut;
    part.add_extrude(i, cut).unwrap();
    // A ring beside it, revolved To the YZ plane (the nearer half).
    let j = part.create_sketch(xy(), &opts).unwrap();
    let sk = &mut part.sketches[j].sketch;
    sk.add_rectangle_two_point(Point2::new(100.0, 0.0), Point2::new(110.0, 10.0))
        .unwrap();
    let rg = sk.regions();
    let mut r = RevolveFeature::new(
        vec![ProfileRef::of(&rg[0], rg[0].interior_point())],
        AxisRef::Origin(1),
    );
    r.extent = RevolveExtent::To;
    r.to = Some(TargetRef::Plane(PlaneRef::Origin(OriginPlane::YZ)));
    r.minimum = true;
    r.operation = Operation::NewSolid;
    part.add_revolve(j, r).unwrap();
    part
}

/// Revision 23: a project with a box part and a drawing of it (a base view and
/// a projected view on an A3 sheet, a second A4 portrait sheet).
fn drawing_r23_project() -> Project {
    use cadcraft3d::document::DocRef;
    use cadcraft3d::drawing::{
        DrawingDocument, Orientation, SheetSize, ViewOrientation, ViewStyle,
    };
    let mut project = Project::new("DrawingR23");
    let part = project.add_document(
        "Part1",
        DocumentData::Part(PartDocument::with_box("Box1", [40.0, 20.0, 10.0]).unwrap()),
    );
    let mut d = DrawingDocument::default();
    let b = d
        .add_base_view(
            DocRef::local(part),
            ViewOrientation::Front,
            [120.0, 180.0],
            2.0,
            ViewStyle::HiddenShown,
        )
        .unwrap();
    d.add_projected_view(b, [120.0, 80.0]).unwrap();
    d.new_sheet();
    d.sheets[1].size = SheetSize::A4;
    d.sheets[1].orientation = Orientation::Portrait;
    d.active_sheet = 0;
    project.add_document("Drawing1", DocumentData::Drawing(d));
    project
}

fn drawing_r23_bytes() -> Vec<u8> {
    let dir = std::env::temp_dir().join(format!("cadcraft-r23-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("drawing_r23.3dcadcraft");
    drawing_r23_project().save(&f).unwrap();
    let bytes = std::fs::read(&f).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    bytes
}

/// The revision 23 drawing reads back with its sheets and views.
#[test]
fn drawing_r23_reads() {
    let f = root().join("r23/drawing_r23.3dcadcraft");
    if !f.exists() {
        return; // written by CADCRAFT_WRITE_COMPAT=1 with the others
    }
    let project = Project::load(&f).unwrap();
    let d = project
        .documents()
        .iter()
        .find_map(|d| match &d.data {
            DocumentData::Drawing(x) => Some(x.clone()),
            _ => None,
        })
        .expect("a drawing");
    assert_eq!(d.sheets.len(), 2);
    assert_eq!(d.sheets[0].views.len(), 2);
    assert_eq!(d.sheets[1].dims(), (210.0, 297.0));
    assert_eq!(d.references().len(), 1);
    project.validate().unwrap();
}

/// Revision 25: an assembly of a plate and three blocks, its drawing with an
/// assembly base view, a parts list (a sort, a cell override, a custom row)
/// and a balloon.
fn drawing_r25_project() -> Project {
    use cadcraft3d::document::{AssemblyDocument, DocRef};
    use cadcraft3d::drawing::parts::{Balloon, CustomRow, PartsList, SortBy};
    use cadcraft3d::drawing::{AnnotationKind, DrawingDocument, ViewOrientation, ViewStyle};
    use cadcraft3d::kernel::Transform3;
    let mut project = Project::new("DrawingR25");
    let plate = project.add_document(
        "Plate",
        DocumentData::Part(PartDocument::with_box("Box1", [80.0, 50.0, 12.0]).unwrap()),
    );
    let block = project.add_document(
        "Block",
        DocumentData::Part(PartDocument::with_box("Box1", [10.0, 10.0, 30.0]).unwrap()),
    );
    let mut asm = AssemblyDocument::default();
    asm.place(DocRef::local(plate), "Plate", Transform3::identity(), true);
    for k in 0..3 {
        let t = Transform3::translation(10.0 + 25.0 * f64::from(k), 20.0, 12.0);
        asm.place(DocRef::local(block), "Block", t, false);
    }
    let a = project.add_document("Assembly1", DocumentData::Assembly(asm));
    let mut d = DrawingDocument::default();
    let v = d
        .add_base_view(
            DocRef::local(a),
            ViewOrientation::Front,
            [120.0, 180.0],
            1.0,
            ViewStyle::HiddenShown,
        )
        .unwrap();
    let mut pl = PartsList::new(v, [405.0, 287.0]);
    pl.sort = Some(SortBy {
        column: "part_number".into(),
        ascending: true,
        renumber: true,
    });
    pl.set_override("Plate", "description", Some("Base plate".into()));
    pl.custom_rows.push(CustomRow {
        cells: [("description".to_owned(), "Grease".to_owned())].into(),
    });
    d.add_annotation(AnnotationKind::PartsList(pl));
    d.add_annotation(AnnotationKind::Balloon(Balloon {
        view: v,
        edge: "o2/Box1:E000-100".into(),
        along: 0.5,
        at: [-40.0, 40.0],
        shape: Default::default(),
        bom: Default::default(),
        text: None,
    }));
    project.add_document("Drawing1", DocumentData::Drawing(d));
    project
}

/// Revision 26: the four-flange tray of revision 22, its drawing with an
/// isometric folded view, a flat pattern view and a bend note on each bend.
fn drawing_r26_project() -> Project {
    use cadcraft3d::document::DocRef;
    use cadcraft3d::drawing::hlr::flat_lines;
    use cadcraft3d::drawing::{
        AnnotationKind, DrawingDocument, SheetView, ViewOrientation, ViewStyle,
    };
    let mut project = Project::new("DrawingR26");
    let part = sheet_r22();
    let lines = flat_lines(&part, &ViewOrientation::Top.frame(), None).unwrap();
    let id = project.add_document("Tray", DocumentData::Part(part));
    let mut d = DrawingDocument::default();
    d.add_base_view(
        DocRef::local(id),
        ViewOrientation::IsoTopRight,
        [110.0, 190.0],
        1.0,
        ViewStyle::HiddenShown,
    )
    .unwrap();
    let flat = d
        .add_base_view(
            DocRef::local(id),
            ViewOrientation::Top,
            [280.0, 160.0],
            1.0,
            ViewStyle::HiddenShown,
        )
        .unwrap();
    d.sheets[0].views[1].sheet_metal = SheetView::Flat;
    for b in &lines.bends {
        d.add_annotation(AnnotationKind::BendNote {
            view: flat,
            bend: b.middle(),
            offset: [0.0, 0.0],
        });
    }
    project.add_document("Drawing1", DocumentData::Drawing(d));
    project
}

/// The revision 26 drawing reads back with its flat pattern view and notes.
#[test]
fn drawing_r26_reads() {
    use cadcraft3d::drawing::{AnnotationKind, SheetView};
    let f = root().join("r26/drawing_r26.3dcadcraft");
    if !f.exists() {
        return; // written by CADCRAFT_WRITE_COMPAT=1 with the others
    }
    let project = Project::load(&f).unwrap();
    let d = project
        .documents()
        .iter()
        .find_map(|d| match &d.data {
            DocumentData::Drawing(x) => Some(x.clone()),
            _ => None,
        })
        .expect("a drawing");
    assert_eq!(d.sheets[0].views[1].sheet_metal, SheetView::Flat);
    let notes = d.sheets[0]
        .annotations
        .iter()
        .filter(|a| matches!(a.kind, AnnotationKind::BendNote { .. }))
        .count();
    assert_eq!(notes, 4);
    project.validate().unwrap();
}

/// The revision 25 drawing reads back with its parts list and balloon.
#[test]
fn drawing_r25_reads() {
    use cadcraft3d::drawing::AnnotationKind;
    let f = root().join("r25/drawing_r25.3dcadcraft");
    if !f.exists() {
        return; // written by CADCRAFT_WRITE_COMPAT=1 with the others
    }
    let project = Project::load(&f).unwrap();
    let d = project
        .documents()
        .iter()
        .find_map(|d| match &d.data {
            DocumentData::Drawing(x) => Some(x.clone()),
            _ => None,
        })
        .expect("a drawing");
    let anns = &d.sheets[0].annotations;
    let Some(AnnotationKind::PartsList(pl)) = anns.first().map(|a| &a.kind) else {
        panic!("a parts list first")
    };
    assert_eq!(pl.overrides.len(), 1);
    assert_eq!(pl.custom_rows.len(), 1);
    assert!(matches!(
        anns.get(1).map(|a| &a.kind),
        Some(AnnotationKind::Balloon(_))
    ));
    project.validate().unwrap();
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
        let f = root().join("r13/combine_split_r13.3dcadcraft");
        if !f.exists() {
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            let mut project = Project::new("CombineSplitR13");
            project.add_document("Part1", DocumentData::Part(combine_split_r13()));
            project.save(&f).unwrap();
        }
        let f = root().join("r11/extrude_options_r11.3dcadcraft");
        if !f.exists() {
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            let mut project = Project::new("ExtrudeOptionsR11");
            project.add_document("Part1", DocumentData::Part(extrude_options_r11()));
            project.save(&f).unwrap();
        }
        let f = root().join("r10/hole_r10.3dcadcraft");
        if !f.exists() {
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            let mut project = Project::new("HoleR10");
            project.add_document("Part1", DocumentData::Part(hole_r10()));
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
        let f = root().join("r23/drawing_r23.3dcadcraft");
        if !f.exists() {
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(&f, drawing_r23_bytes()).unwrap();
        }
        let f = root().join("r25/drawing_r25.3dcadcraft");
        if !f.exists() {
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            drawing_r25_project().save(&f).unwrap();
        }
        let f = root().join("r26/drawing_r26.3dcadcraft");
        if !f.exists() {
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            drawing_r26_project().save(&f).unwrap();
        }
        let f = root().join("r22/sheet_r22.3dcadcraft");
        if !f.exists() {
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            let mut project = Project::new("SheetR22");
            project.add_document("Part1", DocumentData::Part(sheet_r22()));
            project.save(&f).unwrap();
        }
        let f = root().join("r21/sheet_r21.3dcadcraft");
        if !f.exists() {
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            let mut project = Project::new("SheetR21");
            project.add_document("Part1", DocumentData::Part(sheet_r21()));
            project.save(&f).unwrap();
        }
        let f = root().join("r20/sheet_r20.3dcadcraft");
        if !f.exists() {
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            let mut project = Project::new("SheetR20");
            project.add_document("Part1", DocumentData::Part(sheet_r20()));
            project.save(&f).unwrap();
        }
        let f = root().join("r19/sheet_r19.3dcadcraft");
        if !f.exists() {
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            let mut project = Project::new("SheetR19");
            project.add_document("Part1", DocumentData::Part(sheet_r19()));
            project.save(&f).unwrap();
        }
        let f = root().join("r18/logic_r18.3dcadcraft");
        if !f.exists() {
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            let mut project = Project::new("LogicR18");
            project.add_document("Part1", DocumentData::Part(logic_r18()));
            project.save(&f).unwrap();
        }
        let f = root().join("r17/units_r17.3dcadcraft");
        if !f.exists() {
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            let mut project = Project::new("UnitsR17");
            project.add_document("Part1", DocumentData::Part(units_r17()));
            project.save(&f).unwrap();
        }
        let f = root().join("r16/project_work_r16.3dcadcraft");
        if !f.exists() {
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            let mut project = Project::new("ProjectWorkR16");
            project.add_document("Part1", DocumentData::Part(project_work_r16()));
            project.save(&f).unwrap();
        }
        let f = root().join("r15/revolve_work_r15.3dcadcraft");
        if !f.exists() {
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            let mut project = Project::new("RevolveWorkR15");
            project.add_document("Part1", DocumentData::Part(revolve_work_r15()));
            project.save(&f).unwrap();
        }
        let f = root().join("r14/coil_rib_r14.3dcadcraft");
        if !f.exists() {
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            let mut project = Project::new("CoilRibR14");
            project.add_document("Part1", DocumentData::Part(coil_rib_r14()));
            project.save(&f).unwrap();
        }
        let f = root().join("r12/sweep_loft_r12.3dcadcraft");
        if !f.exists() {
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            let mut project = Project::new("SweepLoftR12");
            project.add_document("Part1", DocumentData::Part(sweep_loft_r12()));
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

/// The face names of a fixture after regeneration (sorted, with repeats:
/// a face split in pieces keeps its name on each).
fn face_names(path: &Path) -> Result<Vec<String>, String> {
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
    let mut names: Vec<String> = part
        .body
        .faces()
        .map(|(_, f)| f.name.clone().unwrap_or_else(|| "?".into()))
        .collect();
    names.sort();
    Ok(names)
}

/// Old files give the same **face names** (`docs/KERNEL_API.md` §3): the
/// fillets, holes, appearances and sketches of a part refer to them, so a
/// kernel that names faces differently breaks old files even with the same
/// volume. Recorded in `tests/compat/names.json` (new fixtures with
/// `CADCRAFT_WRITE_COMPAT=1`; never edit recorded entries).
#[test]
fn face_names_stay() {
    let write = std::env::var_os("CADCRAFT_WRITE_COMPAT").is_some();
    let path = root().join("names.json");
    let mut recorded: BTreeMap<String, Vec<String>> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let mut files: Vec<PathBuf> = std::fs::read_dir(root())
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .flat_map(|d| {
            std::fs::read_dir(d)
                .unwrap()
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("3dcadcraft"))
                .collect::<Vec<_>>()
        })
        .collect();
    files.sort();
    let mut problems = Vec::new();
    let mut added = false;
    for f in &files {
        let key = f
            .strip_prefix(root())
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let got = match face_names(f) {
            Ok(n) => n,
            Err(e) => {
                problems.push(format!("{key}: {e}"));
                continue;
            }
        };
        match recorded.get(&key) {
            Some(want) if *want == got => {}
            Some(want) => {
                let lost: Vec<&String> = want.iter().filter(|n| !got.contains(n)).collect();
                let new: Vec<&String> = got.iter().filter(|n| !want.contains(n)).collect();
                problems.push(format!("{key}: names changed; lost {lost:?}, new {new:?}"));
            }
            None if write => {
                recorded.insert(key, got);
                added = true;
            }
            None => problems.push(format!(
                "{key}: names not recorded (CADCRAFT_WRITE_COMPAT=1)"
            )),
        }
    }
    if added {
        std::fs::write(&path, serde_json::to_string_pretty(&recorded).unwrap()).unwrap();
    }
    assert!(
        problems.is_empty(),
        "face names changed:\n{}",
        problems.join("\n")
    );
}
