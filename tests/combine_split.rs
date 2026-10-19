//! Combine and Split (`docs/spec/2.13-combine-split.md`): analytic
//! volumes, body and face names, Keep Toolbody, deleting the feature,
//! appearance, refusals, Split Face → Face Draft / Shell, Split Solid →
//! Fillet, a shelled box, a chamfer cone (SolidCraft #1691), save / load.

mod common;

use cadcraft3d::part::appearance::LookTarget;
use cadcraft3d::part::{
    CombineFeature, LineRef, SplitFeature, SplitMethod, SplitToolRef, WorkDef, WorkFeature,
    WorkPlaneDef,
};
use common::*;

// ------------------------------------------------------------ helpers ---

/// A box `[x0, x1] × [y0, y1] × [0, h]` as a New Solid extrusion.
fn block(part: &mut PartDocument, x0: Real, y0: Real, x1: Real, y1: Real, h: Real) -> String {
    let i = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    let sk = &mut part.sketches[i].sketch;
    sk.add_rectangle_two_point(Point2::new(x0, y0), Point2::new(x1, y1))
        .unwrap();
    let r = sk.regions();
    part.add_extrude(i, ExtrudeFeature::new(region_profiles(&r, |_| true), h))
        .unwrap()
}

/// A cylinder of radius `r` at (cx, cy), height `h`, as a New Solid.
fn cylinder(part: &mut PartDocument, cx: Real, cy: Real, r: Real, h: Real) -> String {
    let i = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    let sk = &mut part.sketches[i].sketch;
    sk.add_circle(Point2::new(cx, cy), r).unwrap();
    let reg = sk.regions();
    part.add_extrude(i, ExtrudeFeature::new(region_profiles(&reg, |_| true), h))
        .unwrap()
}

fn combine(base: &str, tools: &[&str], op: Operation, keep: bool) -> CombineFeature {
    let mut c = CombineFeature::new();
    c.base = base.into();
    c.tools = tools.iter().map(|t| t.to_string()).collect();
    c.operation = op;
    c.keep_tools = keep;
    c
}

fn split(method: SplitMethod, tool: SplitToolRef) -> SplitFeature {
    let mut s = SplitFeature::new(method);
    s.tool = Some(tool);
    s
}

fn origin(p: OriginPlane) -> SplitToolRef {
    SplitToolRef::Plane(PlaneRef::Origin(p))
}

/// A work plane offset `d` from origin plane `p`.
fn offset_plane(part: &mut PartDocument, p: OriginPlane, d: Real) -> SplitToolRef {
    let id = part
        .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
            base: PlaneRef::Origin(p),
            distance: d,
        })))
        .unwrap();
    SplitToolRef::Plane(PlaneRef::Work(id))
}

/// The volumes of the solids by body name.
fn volumes(part: &PartDocument) -> Vec<(String, Real)> {
    part.exact_bodies
        .0
        .iter()
        .map(|b| {
            (
                b.name.clone().unwrap_or_default(),
                b.measure_full().map(|m| m.volume).unwrap_or(0.0),
            )
        })
        .collect()
}

fn centroid(part: &PartDocument, name: &str) -> Point3 {
    part.exact_bodies
        .0
        .iter()
        .find(|b| b.name.as_deref() == Some(name))
        .and_then(|b| b.measure_full().ok())
        .map(|m| m.centroid)
        .expect("no such body")
}

fn vol_of(part: &PartDocument, name: &str) -> Real {
    volumes(part)
        .into_iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v)
        .unwrap_or_else(|| panic!("no body {name}: {:?}", volumes(part)))
}

fn close(v: Real, want: Real, rel: Real) {
    assert!(
        (v - want).abs() <= rel * want.abs().max(1.0),
        "volume {v} vs {want}"
    );
}

/// Two overlapping boxes: A = 40×30×20 at the origin, B = [30, 60] × [0, 30]
/// × [0, 20] (overlap 10×30×20 = 6000).
fn two_boxes() -> (PartDocument, String, String) {
    let mut part = PartDocument::default();
    let a = block(&mut part, 0.0, 0.0, 40.0, 30.0, 20.0);
    let b = block(&mut part, 30.0, 0.0, 60.0, 30.0, 20.0);
    (part, a, b)
}

/// A box centred on the origin: [−20, 20] × [−15, 15] × [0, 20] = 24000.
fn centred() -> PartDocument {
    let mut part = PartDocument::default();
    block(&mut part, -20.0, -15.0, 20.0, 15.0, 20.0);
    part
}

// ------------------------------------------------------------ Combine ---

#[test]
fn combine_join_cut_intersect_two_bodies() {
    for (op, want) in [
        (Operation::Join, 36000.0),
        (Operation::Cut, 18000.0),
        (Operation::Intersect, 6000.0),
    ] {
        let (mut part, a, b) = two_boxes();
        let id = part.add_combine(combine(&a, &[&b], op, false)).unwrap();
        assert_eq!(id, "Combine1");
        check(&part, want, 1e-6);
        // One body left, the base's name.
        assert_eq!(part.exact_bodies.0.len(), 1, "{op:?}");
        assert_eq!(part.exact_bodies.0[0].name.as_deref(), Some(a.as_str()));
        assert_eq!(part.bodies_before(usize::MAX).len(), 1, "{op:?}");
    }
}

#[test]
fn combine_four_bodies() {
    let build = || {
        let mut part = PartDocument::default();
        let a = block(&mut part, 0.0, 0.0, 40.0, 30.0, 20.0);
        let b = block(&mut part, 30.0, 0.0, 60.0, 30.0, 20.0);
        let c = block(&mut part, 10.0, 20.0, 20.0, 40.0, 20.0);
        let d = cylinder(&mut part, 25.0, 10.0, 4.0, 30.0);
        (part, a, b, c, d)
    };
    let cyl = PI * 16.0;
    // Join: A ∪ B ∪ C ∪ D.
    let (mut part, a, b, c, d) = build();
    part.add_combine(combine(&a, &[&b, &c, &d], Operation::Join, false))
        .unwrap();
    check(&part, 24000.0 + 12000.0 + 2000.0 + cyl * 10.0, 1e-4);
    assert_eq!(part.exact_bodies.0.len(), 1);
    // Cut: A − B − C − D.
    let (mut part, a, b, c, d) = build();
    part.add_combine(combine(&a, &[&b, &c, &d], Operation::Cut, false))
        .unwrap();
    check(&part, 24000.0 - 6000.0 - 2000.0 - cyl * 20.0, 1e-4);
    // Intersect A with D: the cylinder inside A.
    let (mut part, a, _, _, d) = build();
    part.add_combine(combine(&a, &[&d], Operation::Intersect, false))
        .unwrap();
    // B and C remain untouched (3 bodies).
    assert_eq!(part.exact_bodies.0.len(), 3);
    close(vol_of(&part, &a), cyl * 20.0, 1e-4);
    // Intersect with B and C (no common volume of all three): refused.
    let (mut part, a, b, c, _) = build();
    refused(
        part.add_combine(combine(&a, &[&b, &c], Operation::Intersect, false)),
        "no volume in common",
    );
}

#[test]
fn keep_toolbody_stays_hidden() {
    let (mut part, a, b) = two_boxes();
    part.add_combine(combine(&a, &[&b], Operation::Join, true))
        .unwrap();
    // The joined base and the kept (hidden) tool.
    assert_eq!(part.exact_bodies.0.len(), 2);
    close(vol_of(&part, &a), 36000.0, 1e-6);
    close(vol_of(&part, &b), 18000.0, 1e-6);
    assert!(!part.body_visible(&b));
    assert!(part.body_visible(&a));
    assert_eq!(part.bodies_before(usize::MAX).len(), 2);
    // Without Keep Toolbody the tool is consumed and not hidden.
    let (mut part, a, b) = two_boxes();
    part.add_combine(combine(&a, &[&b], Operation::Cut, false))
        .unwrap();
    assert!(part.hidden_bodies.is_empty());
    assert_eq!(part.exact_bodies.0.len(), 1);
}

#[test]
fn deleting_combine_restores_the_tools() {
    let (mut part, a, b) = two_boxes();
    part.add_combine(combine(&a, &[&b], Operation::Cut, false))
        .unwrap();
    check(&part, 18000.0, 1e-6);
    let k = part.features.len() - 1;
    assert!(part.breaks_if_removed(k).is_empty());
    part.remove_feature(k).unwrap();
    part.regenerate().unwrap();
    assert_eq!(part.exact_bodies.0.len(), 2);
    check(&part, 42000.0, 1e-6);
    assert_eq!(part.bodies_before(usize::MAX).len(), 2);
}

#[test]
fn combine_keeps_the_base_appearance() {
    let lib = cadcraft3d::materials::builtin();
    let (mut part, a, b) = two_boxes();
    part.set_appearance(&LookTarget::Body(a.clone()), Some("ABS - Blue"), lib)
        .unwrap();
    part.set_appearance(&LookTarget::Body(b.clone()), Some("ABS - Red"), lib)
        .unwrap();
    part.add_combine(combine(&a, &[&b], Operation::Join, false))
        .unwrap();
    let looks = cadcraft3d::part::appearance::Resolver::new(&part).face_looks();
    assert!(!looks.is_empty());
    // Every face of the result (the tool's faces too) is blue.
    for (f, (look, _)) in &looks {
        assert_eq!(look.name, "ABS - Blue", "{:?}", part.body.face(*f).and_then(|x| x.name.clone()));
    }
}

#[test]
fn combine_refusals_leave_the_part_unchanged() {
    let (mut part, a, b) = two_boxes();
    let n = part.features.len();
    let v = volume(&part);
    refused(
        part.add_combine(combine(&a, &[&a], Operation::Join, false)),
        "cannot also be a toolbody",
    );
    refused(
        part.add_combine(combine(&a, &[], Operation::Join, false)),
        "at least one toolbody",
    );
    refused(
        part.add_combine(combine("", &[&b], Operation::Join, false)),
        "base body",
    );
    refused(
        part.add_combine(combine(&a, &[&b], Operation::NewSolid, false)),
        "Join, Cut or Intersect",
    );
    refused(
        part.add_combine(combine(&a, &["Nothing"], Operation::Join, false)),
        "no longer exists",
    );
    // Cut by a body apart from the base.
    let c = block(&mut part, 100.0, 0.0, 110.0, 10.0, 10.0);
    let n2 = part.features.len();
    refused(
        part.add_combine(combine(&a, &[&c], Operation::Cut, false)),
        "does not intersect",
    );
    assert_eq!(part.features.len(), n2);
    assert_eq!(n + 1, n2);
    close(volume(&part), v + 1000.0, 1e-9);
    assert!(part.feature_health.is_empty());
}

#[test]
fn combine_edit_feature_changes_the_operation() {
    let (mut part, a, b) = two_boxes();
    part.add_combine(combine(&a, &[&b], Operation::Join, false))
        .unwrap();
    let k = part.features.len() - 1;
    let Some(Feature::Combine(mut c)) = part.features.get(k).cloned() else {
        panic!("not a combine")
    };
    c.operation = Operation::Intersect;
    part.replace_feature(k, Feature::Combine(c)).unwrap();
    check(&part, 6000.0, 1e-6);
    let Some(Feature::Combine(c)) = part.features.get(k) else {
        panic!("not a combine")
    };
    assert_eq!(c.id, "Combine1");
}

#[test]
fn combine_then_fillet_and_shell() {
    // Join two boxes, then shell the joined body from its top: the tool's
    // faces work as inputs after the Combine.
    let (mut part, a, b) = two_boxes();
    part.add_combine(combine(&a, &[&b], Operation::Join, false))
        .unwrap();
    let topa = face_where(&part, |p| (p.z - 20.0).abs() < 1e-6);
    let mut sh = ShellFeature::new(2.0);
    sh.remove.push(face_ref(&part, &topa, [5.0, 15.0, 20.0]));
    // Every top piece is one face of the union (or two coplanar pieces).
    for (fid, f) in part.body.faces() {
        let _ = fid;
        if let Some(n) = &f.name
            && n != &topa
            && f.mesh.as_ref().is_some_and(|m| m.positions.iter().all(|p| (p.z - 20.0).abs() < 1e-6))
        {
            sh.remove.push(face_ref(&part, n, [55.0, 15.0, 20.0]));
        }
    }
    part.add_shell(sh).unwrap();
    // 60×30×20 shelled 2 from the top: 36000 − 56·26·18.
    check(&part, 36000.0 - 56.0 * 26.0 * 18.0, 1e-4);
}

// -------------------------------------------------------------- Split ---

#[test]
fn split_solid_by_origin_plane() {
    let mut part = centred();
    let id = part
        .add_split(split(SplitMethod::SplitSolid, origin(OriginPlane::YZ)))
        .unwrap();
    assert_eq!(id, "Split1");
    check(&part, 24000.0, 1e-6);
    assert_eq!(part.exact_bodies.0.len(), 2);
    close(vol_of(&part, "Extrusion1"), 12000.0, 1e-6);
    close(vol_of(&part, "Split1#2"), 12000.0, 1e-6);
    // The back part (x < 0) keeps the body's name.
    assert!(centroid(&part, "Extrusion1").x < -1.0);
    assert!(centroid(&part, "Split1#2").x > 1.0);
    // Section faces: one per part, different names.
    assert!(part.body.find_face("Split1:Cut").is_some());
    assert!(part.body.find_face("Split1#2:Cut").is_some());
    // The new body is listed (Solid2).
    let names = part.bodies_before(usize::MAX);
    assert_eq!(
        names,
        vec![
            ("Extrusion1".to_owned(), "Solid1".to_owned()),
            ("Split1#2".to_owned(), "Solid2".to_owned())
        ]
    );
}

#[test]
fn split_solid_by_xz_and_offset_planes() {
    let mut part = centred();
    part.add_split(split(SplitMethod::SplitSolid, origin(OriginPlane::XZ)))
        .unwrap();
    check(&part, 24000.0, 1e-6);
    close(vol_of(&part, "Split1#2"), 12000.0, 1e-6);
    // An offset work plane at z = 5: 40×30×5 and 40×30×15.
    let mut part = centred();
    let t = offset_plane(&mut part, OriginPlane::XY, 5.0);
    part.add_split(split(SplitMethod::SplitSolid, t)).unwrap();
    check(&part, 24000.0, 1e-6);
    close(vol_of(&part, "Extrusion1"), 6000.0, 1e-6);
    close(vol_of(&part, "Split2#2"), 18000.0, 1e-6);
}

#[test]
fn split_solid_by_a_plane_at_an_angle() {
    // A work plane through the Z axis at 30° from XZ: through the box's
    // centre line, so both halves are 12000 (central symmetry).
    let mut part = centred();
    let id = part
        .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Angle {
            base: PlaneRef::Origin(OriginPlane::XZ),
            axis: LineRef::Origin(2),
            angle: 30f64.to_radians(),
        })))
        .unwrap();
    part.add_split(split(
        SplitMethod::SplitSolid,
        SplitToolRef::Plane(PlaneRef::Work(id)),
    ))
    .unwrap();
    check(&part, 24000.0, 1e-6);
    close(vol_of(&part, "Extrusion1"), 12000.0, 1e-6);
    close(vol_of(&part, "Split2#2"), 12000.0, 1e-6);
}

/// A sketch on XY with one line from `a` to `b`.
fn line_sketch(part: &mut PartDocument, a: (Real, Real), b: (Real, Real)) -> String {
    let i = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    part.sketches[i]
        .sketch
        .add_line(Point2::new(a.0, a.1), Point2::new(b.0, b.1));
    part.sketches[i].id.clone()
}

#[test]
fn split_solid_by_a_sketch_line() {
    // x = 10, drawn upwards: the front (sketch normal × direction = −X) is
    // x < 10 (30 wide), the back x > 10 (10 wide).
    let mut part = centred();
    let s = line_sketch(&mut part, (10.0, -40.0), (10.0, 40.0));
    part.add_split(split(SplitMethod::SplitSolid, SplitToolRef::Sketch(s.clone())))
        .unwrap();
    check(&part, 24000.0, 1e-6);
    close(vol_of(&part, "Extrusion1"), 6000.0, 1e-6);
    close(vol_of(&part, "Split1#2"), 18000.0, 1e-6);
    // The sketch is consumed (hidden, nested under the Split).
    let ps = part.sketches.iter().find(|p| p.id == s).unwrap();
    assert!(!ps.visible);
    assert_eq!(part.consumers(&s).len(), 1);
    // A diagonal line through the centre: halves.
    let mut part = centred();
    let s = line_sketch(&mut part, (-30.0, -30.0), (30.0, 30.0));
    part.add_split(split(SplitMethod::SplitSolid, SplitToolRef::Sketch(s)))
        .unwrap();
    close(vol_of(&part, "Extrusion1"), 12000.0, 1e-6);
}

#[test]
fn split_solid_by_a_closed_profile() {
    // A circle r = 5 at the centre: a core cylinder and the box less it.
    let mut part = centred();
    let i = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    part.sketches[i]
        .sketch
        .add_circle(Point2::new(0.0, 0.0), 5.0)
        .unwrap();
    let s = part.sketches[i].id.clone();
    part.add_split(split(SplitMethod::SplitSolid, SplitToolRef::Sketch(s)))
        .unwrap();
    let core = PI * 25.0 * 20.0;
    check(&part, 24000.0, 1e-5);
    close(vol_of(&part, "Split1#2"), core, 1e-4);
    close(vol_of(&part, "Extrusion1"), 24000.0 - core, 1e-4);
}

#[test]
fn trim_solid_both_sides() {
    let mut part = centred();
    part.add_split(split(SplitMethod::TrimSolid, origin(OriginPlane::YZ)))
        .unwrap();
    check(&part, 12000.0, 1e-6);
    assert_eq!(part.exact_bodies.0.len(), 1);
    // Default: the front (x > 0, where the arrow points) is removed.
    assert!(centroid(&part, "Extrusion1").x < -1.0);
    assert!(part.body.find_face("Split1:Cut").is_some());
    let mut part = centred();
    let mut sp = split(SplitMethod::TrimSolid, origin(OriginPlane::YZ));
    sp.flip = true;
    part.add_split(sp).unwrap();
    check(&part, 12000.0, 1e-6);
    assert!(centroid(&part, "Extrusion1").x > 1.0);
    // The kept part's section is `<id>:Cut` either way.
    assert!(part.body.find_face("Split1:Cut").is_some());
    assert!(part.body.find_face("Split1#2:Cut").is_none());
    // Offset plane z = 5, flipped: the top 15 stays.
    let mut part = centred();
    let t = offset_plane(&mut part, OriginPlane::XY, 5.0);
    let mut sp = split(SplitMethod::TrimSolid, t);
    sp.flip = true;
    part.add_split(sp).unwrap();
    check(&part, 18000.0, 1e-6);
}

#[test]
fn split_face_all_faces_and_names() {
    let mut part = centred();
    let before = part.body.faces().count();
    let t = offset_plane(&mut part, OriginPlane::XY, 10.0);
    let mut sp = split(SplitMethod::SplitFace, t);
    sp.all_faces = true;
    part.add_split(sp).unwrap();
    // The solid is unchanged; the four side faces are in two pieces each.
    check(&part, 24000.0, 1e-6);
    assert_eq!(part.body.faces().count(), before + 4);
    let names: Vec<String> = part.body.faces().filter_map(|(_, f)| f.name.clone()).collect();
    let pieces = names.iter().filter(|n| n.contains("~Split2.")).count();
    assert_eq!(pieces, 8, "{names:?}");
    // `.1` is below the plane (its back), `.2` above.
    for (_, f) in part.body.faces() {
        let (Some(n), Some(m)) = (&f.name, &f.mesh) else { continue };
        if n.ends_with("~Split2.1") {
            assert!(m.positions.iter().all(|p| p.z <= 10.0 + 1e-6), "{n}");
        }
        if n.ends_with("~Split2.2") {
            assert!(m.positions.iter().all(|p| p.z >= 10.0 - 1e-6), "{n}");
        }
    }
}

#[test]
fn split_face_by_a_sketch_circle_on_the_top() {
    let mut part = centred();
    let top = face_where(&part, |p| (p.z - 20.0).abs() < 1e-6);
    let before = part.body.faces().count();
    let j = sketch_on(&mut part, &top, |sk, at| {
        sk.add_circle(at(0.0, 0.0, 20.0), 6.0).unwrap();
    });
    let s = part.sketches[j].id.clone();
    let mut sp = split(SplitMethod::SplitFace, SplitToolRef::Sketch(s));
    sp.faces.push(face_ref(&part, &top, [10.0, 10.0, 20.0]));
    sp.keep_sketch_visible = true;
    part.add_split(sp).unwrap();
    check(&part, 24000.0, 1e-6);
    assert_eq!(part.body.faces().count(), before + 1);
    assert!(part.sketches[j].visible, "Keep sketch visible");
}

/// The front face (y = −15) of the centred box split at z = 10, the upper
/// piece drafted 5° about the split line (pull +Z): a wedge 40 × 10² tan 5°
/// / 2 leaves.
#[test]
fn split_face_then_face_draft_on_one_piece() {
    let mut part = centred();
    let front = face_where(&part, |p| (p.y + 15.0).abs() < 1e-6);
    let t = offset_plane(&mut part, OriginPlane::XY, 10.0);
    let SplitToolRef::Plane(wp) = t.clone() else {
        panic!()
    };
    let mut sp = split(SplitMethod::SplitFace, t);
    sp.faces.push(face_ref(&part, &front, [0.0, -15.0, 5.0]));
    part.add_split(sp).unwrap();
    let upper = format!("{front}~Split2.2");
    assert!(part.body.find_face(&upper).is_some());
    let mut d = DraftFeature::new(5f64.to_radians());
    d.draft_type = DraftType::FixedPlane;
    d.plane = Some(wp);
    d.faces.push(face_ref(&part, &upper, [0.0, -15.0, 15.0]));
    part.add_draft(d).unwrap();
    let wedge = 40.0 * 100.0 * 5f64.to_radians().tan() / 2.0;
    let v = volume(&part);
    println!("split + draft: {v} (24000 ± {wedge})");
    // Leaning in (material removed) for a positive angle.
    check(&part, 24000.0 - wedge, 1e-4);
}

#[test]
fn split_face_then_shell_without_one_piece() {
    // The top of the 40×30×20 plate split at x = 20; Shell 2 removing the
    // left piece only: the cavity, and the opening over the left piece.
    let mut part = plate(40.0, 30.0, 20.0);
    let top = top(&part, 20.0);
    let t = offset_plane(&mut part, OriginPlane::YZ, 20.0);
    let mut sp = split(SplitMethod::SplitFace, t);
    sp.faces.push(face_ref(&part, &top, [10.0, 15.0, 20.0]));
    part.add_split(sp).unwrap();
    let left = format!("{top}~Split2.1");
    let mut sh = ShellFeature::new(2.0);
    sh.remove.push(face_ref(&part, &left, [10.0, 15.0, 20.0]));
    match part.add_shell(sh) {
        Ok(_) => {
            let v = volume(&part);
            println!("split face + shell: {v}");
            assert!(part.feature_health.is_empty());
            // Between a closed shell (7152 + top wall) and an open one.
            assert!(v > 24000.0 - 36.0 * 26.0 * 18.0 - 1.0 && v < 24000.0 - 36.0 * 26.0 * 16.0 + 1.0, "{v}");
        }
        // A kernel refusal is the feature's error, the part unchanged.
        Err(e) => {
            println!("shell of one piece refused: {e}");
            check(&part, 24000.0, 1e-6);
        }
    }
}

#[test]
fn split_solid_then_fillet_a_section_edge() {
    let mut part = centred();
    part.add_split(split(SplitMethod::SplitSolid, origin(OriginPlane::YZ)))
        .unwrap();
    // The top edge of the front part's section (x = 0, z = 20, along y).
    let edges: Vec<EdgeRef> = edges_where(&part, |p| {
        p[0].abs() < 1e-6 && (p[2] - 20.0).abs() < 1e-6
    })
    .into_iter()
    .filter(|e| e.faces.iter().any(|f| f == "Split1#2:Cut"))
    .collect();
    assert_eq!(edges.len(), 1, "{edges:?}");
    part.add_blend(BlendFeature::fillet(edges, 2.0)).unwrap();
    check(&part, 24000.0 - round_loss(2.0) * 30.0, 1e-4);
    close(vol_of(&part, "Extrusion1"), 12000.0, 1e-6);
}

#[test]
fn split_a_shelled_box() {
    let mut part = plate(40.0, 30.0, 20.0);
    let t = top(&part, 20.0);
    shell(&mut part, &[(&t, [20.0, 15.0, 20.0])], 2.0, ShellDirection::Inside);
    let shelled = 24000.0 - 36.0 * 26.0 * 18.0;
    check(&part, shelled, 1e-6);
    let tool = offset_plane(&mut part, OriginPlane::YZ, 20.0);
    part.add_split(split(SplitMethod::SplitSolid, tool)).unwrap();
    check(&part, shelled, 1e-6);
    close(vol_of(&part, "Extrusion1"), shelled / 2.0, 1e-6);
    close(vol_of(&part, "Split2#2"), shelled / 2.0, 1e-6);
}

/// SolidCraft #1691: a split through the cone of a chamfer lost 0.6 % of
/// the volume. The parts must add up to the whole.
#[test]
fn split_across_a_chamfer_cone_keeps_the_volume() {
    let build = || {
        let mut part = PartDocument::default();
        cylinder(&mut part, 0.0, 0.0, 10.0, 20.0);
        let rim: Vec<EdgeRef> = edges_where(&part, |p| (p[2] - 20.0).abs() < 1e-6);
        assert_eq!(rim.len(), 1);
        part.add_blend(BlendFeature::chamfer(rim, 2.0)).unwrap();
        part
    };
    let part = build();
    // Cylinder less the chamfer ring: π(r²h) − the ring's 2π r̄ A.
    let whole = volume(&part);
    let ring = 2.0 * PI * (10.0 - 2.0 / 3.0) * 2.0;
    close(whole, PI * 100.0 * 20.0 - ring, 1e-4);
    // Through the axis, and across the cone at z = 19 and at an angle.
    for (plane, z) in [(OriginPlane::XZ, None), (OriginPlane::XY, Some(19.0))] {
        let mut part = build();
        let tool = match z {
            None => origin(plane),
            Some(z) => offset_plane(&mut part, plane, z),
        };
        part.add_split(split(SplitMethod::SplitSolid, tool)).unwrap();
        let v = volumes(&part);
        let sum: Real = v.iter().map(|x| x.1).sum();
        println!("#1691 {plane:?} {z:?}: {v:?}, sum {sum} vs {whole}");
        assert_eq!(v.len(), 2);
        close(sum, whole, 1e-5);
    }
}

#[test]
fn split_refusals_leave_the_part_unchanged() {
    let mut part = centred();
    let n = part.features.len();
    // No tool.
    refused(
        part.add_split(SplitFeature::new(SplitMethod::SplitSolid)),
        "select the split tool",
    );
    // A plane that misses the body.
    let far = offset_plane(&mut part, OriginPlane::XY, 50.0);
    let n = n + 1;
    refused(
        part.add_split(split(SplitMethod::SplitSolid, far.clone())),
        "does not split",
    );
    refused(
        part.add_split(split(SplitMethod::TrimSolid, far.clone())),
        "does not split",
    );
    // Split Face: a face the tool does not cross is named.
    let front = face_where(&part, |p| (p.y + 15.0).abs() < 1e-6);
    let mut sp = split(SplitMethod::SplitFace, far);
    sp.faces.push(face_ref(&part, &front, [0.0, -15.0, 5.0]));
    refused(part.add_split(sp), "does not cross");
    // A surface body tool: not yet.
    refused(
        part.add_split(split(
            SplitMethod::SplitSolid,
            SplitToolRef::Surface("Srf1".into()),
        )),
        "not supported yet",
    );
    // An open chain of two lines cannot split a solid (Split Face only).
    let i = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    let sk = &mut part.sketches[i].sketch;
    sk.add_line(Point2::new(-30.0, 0.0), Point2::new(0.0, 0.0));
    let l2 = sk.add_line(Point2::new(0.0, 0.0), Point2::new(0.0, 30.0));
    let _ = l2;
    let s = part.sketches[i].id.clone();
    refused(
        part.add_split(split(SplitMethod::SplitSolid, SplitToolRef::Sketch(s))),
        "Split Face only",
    );
    assert_eq!(part.features.len(), n);
    check(&part, 24000.0, 1e-6);
    assert_eq!(part.exact_bodies.0.len(), 1);
}

#[test]
fn split_face_by_an_open_chain() {
    // Two lines joined at a corner (an L) on the top face: one piece cut
    // out of the top, the rest kept (Split Face follows the chain).
    let mut part = centred();
    let top = face_where(&part, |p| (p.z - 20.0).abs() < 1e-6);
    let before = part.body.faces().count();
    let j = sketch_on(&mut part, &top, |sk, at| {
        let (a, b, c) = (at(-30.0, 0.0, 20.0), at(0.0, 0.0, 20.0), at(0.0, 30.0, 20.0));
        let l1 = sk.add_line(a, b);
        let l2 = sk.add_line(b, c);
        // Share the corner point.
        let _ = (l1, l2);
    });
    let s = part.sketches[j].id.clone();
    let mut sp = split(SplitMethod::SplitFace, SplitToolRef::Sketch(s));
    sp.faces.push(face_ref(&part, &top, [10.0, -10.0, 20.0]));
    match part.add_split(sp) {
        Ok(_) => {
            check(&part, 24000.0, 1e-6);
            assert!(part.body.faces().count() > before, "no new face");
        }
        Err(e) => panic!("open chain: {e}"),
    }
}

#[test]
fn combine_and_split_save_and_load() {
    let (mut part, a, b) = two_boxes();
    part.add_combine(combine(&a, &[&b], Operation::Join, true))
        .unwrap();
    part.add_split(split(SplitMethod::SplitSolid, origin(OriginPlane::YZ)))
        .ok();
    let tool = offset_plane(&mut part, OriginPlane::YZ, 20.0);
    part.add_split(split(SplitMethod::SplitSolid, tool)).unwrap();
    let v = volume(&part);
    let bodies = part.exact_bodies.0.len();
    let dir = std::env::temp_dir().join(format!("cadcraft_combine_split_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("cs.3dcadcraft");
    let mut project = Project::new("CS");
    project.add_document("Part1", DocumentData::Part(part.clone()));
    project.save(&f).unwrap();
    let mut back = Project::load(&f).unwrap();
    let id = back.documents()[0].id;
    let Some(DocumentData::Part(p)) = back.document_mut(id).map(|d| &mut d.data) else {
        panic!("no part")
    };
    p.regenerate().unwrap();
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    close(volume(p), v, 1e-9);
    assert_eq!(p.exact_bodies.0.len(), bodies);
    assert_eq!(p.hidden_bodies, part.hidden_bodies);
    assert_eq!(p.features, part.features);
    let _ = std::fs::remove_dir_all(&dir);
}
