//! Split by surfaces and open sketch curves (`docs/spec/2.13-combine-split.md`):
//! exact surface bodies from Extrude / Revolve with Output Surface, Split Solid
//! / Trim Solid / Split Face with them, open splines and curve chains for
//! Split Solid / Trim Solid, the owner's session (`tests/data/
//! split_spline_journal.jsonl`), refusals, save / load.

mod common;

use cadcraft3d::kernel::exact::ExactSolid;
use cadcraft3d::part::{
    LineRef, OpenProfile, Output, RevolveExtent, SplitFeature, SplitMethod, SplitToolRef, WorkDef,
    WorkFeature, WorkPlaneDef,
};
use cadcraft3d::sketch::EntityId;
use common::*;

// ------------------------------------------------------------ helpers ---

/// A box `[x0, x1] × [y0, y1] × [z0, z0 + h]` (a symmetric extrusion when
/// `sym`, centred on z = 0).
fn block(part: &mut PartDocument, x: (Real, Real), y: (Real, Real), h: Real, sym: bool) {
    let i = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    let sk = &mut part.sketches[i].sketch;
    sk.add_rectangle_two_point(Point2::new(x.0, y.0), Point2::new(x.1, y.1))
        .unwrap();
    let r = sk.regions();
    let mut e = ExtrudeFeature::new(region_profiles(&r, |_| true), h);
    if sym {
        e.direction = ExtentDirection::Symmetric;
    }
    part.add_extrude(i, e).unwrap();
}

/// The centred box [−20, 20] × [−15, 15] × [0, 20] = 24000.
fn centred() -> PartDocument {
    let mut part = PartDocument::default();
    block(&mut part, (-20.0, 20.0), (-15.0, 15.0), 20.0, false);
    part
}

/// The closed solids' volumes by body name (surface bodies left out).
fn volumes(part: &PartDocument) -> Vec<(String, Real)> {
    part.exact_bodies
        .0
        .iter()
        .filter(|b| !b.is_open())
        .map(|b| {
            (
                b.name.clone().unwrap_or_default(),
                // A fine mesh (curved sections: the default one is ~1e-4 off).
                b.volume_at(1e-3).unwrap_or(0.0),
            )
        })
        .collect()
}

fn total(part: &PartDocument) -> Real {
    volumes(part).iter().map(|v| v.1).sum()
}

fn vol_of(part: &PartDocument, name: &str) -> Real {
    volumes(part)
        .into_iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v)
        .unwrap_or_else(|| panic!("no body {name}: {:?}", volumes(part)))
}

fn centroid(part: &PartDocument, name: &str) -> Point3 {
    part.exact_bodies
        .0
        .iter()
        .find(|b| !b.is_open() && b.name.as_deref() == Some(name))
        .and_then(|b| b.measure_full().ok())
        .map(|m| m.centroid)
        .expect("no such body")
}

fn close(v: Real, want: Real, rel: Real) {
    assert!(
        (v - want).abs() <= rel * want.abs().max(1.0),
        "volume {v} vs {want}"
    );
}

/// Exact, healthy, valid solids (surfaces are open, not checked).
fn healthy(part: &PartDocument) {
    assert!(part.exact, "built with the faceted kernel");
    assert!(
        part.feature_health.is_empty(),
        "sick: {:?}",
        part.feature_health
    );
    for (sid, s) in part.body.solids() {
        if !s.open {
            part.body
                .validate_solid(sid)
                .unwrap_or_else(|e| panic!("invalid solid: {e:?}"));
        }
    }
}

fn split(method: SplitMethod, tool: SplitToolRef) -> SplitFeature {
    let mut s = SplitFeature::new(method);
    s.tool = Some(tool);
    s
}

/// A surface extruded symmetrically (`len` in all) from open curves that
/// `draw` adds to a sketch on origin plane `on`.
fn surface(
    part: &mut PartDocument,
    on: OriginPlane,
    len: Real,
    draw: impl FnOnce(&mut cadcraft3d::sketch::Sketch) -> Vec<EntityId>,
) -> String {
    let i = part
        .create_sketch(SketchPlacement::on_origin_plane(on), &opts())
        .unwrap();
    let curves = draw(&mut part.sketches[i].sketch);
    let mut e = ExtrudeFeature::new(Vec::new(), len);
    e.direction = ExtentDirection::Symmetric;
    e.output = Output::Surface;
    e.operation = Operation::NewSolid;
    e.open = Some(OpenProfile {
        curves,
        flip: false,
        match_shape: false,
    });
    part.add_extrude(i, e).unwrap()
}

/// The plane y = 5 as an extruded surface (a line on XY, ±30 along Z).
fn plane_y5(part: &mut PartDocument) -> String {
    surface(part, OriginPlane::XY, 60.0, |sk| {
        vec![sk.add_line(Point2::new(-40.0, 5.0), Point2::new(40.0, 5.0))]
    })
}

/// An arc about (0, −60), radius 70, from 60° to 120°, extruded ±30.
fn arc_surface(part: &mut PartDocument) -> String {
    surface(part, OriginPlane::XY, 60.0, |sk| {
        let c = sk.add_point(Point2::new(0.0, -60.0));
        let at = |deg: Real| {
            let t = deg.to_radians();
            Point2::new(70.0 * t.cos(), -60.0 + 70.0 * t.sin())
        };
        let a = sk.add_point(at(60.0));
        let b = sk.add_point(at(120.0));
        vec![sk.add_arc(c, a, b).unwrap()]
    })
}

fn sheets(part: &PartDocument) -> Vec<&ExactSolid> {
    part.exact_bodies.0.iter().filter(|b| b.is_open()).collect()
}

// ------------------------------------------------- exact surface bodies ---

#[test]
fn surface_output_is_exact_and_open() {
    let mut part = centred();
    let id = plane_y5(&mut part);
    healthy(&part);
    assert_eq!(id, "Extrusion2");
    // The surface is an exact open body named after its feature; the box is
    // untouched (no Boolean).
    assert_eq!(sheets(&part).len(), 1);
    assert_eq!(sheets(&part)[0].name.as_deref(), Some("Extrusion2"));
    close(total(&part), 24000.0, 1e-9);
    let open: Vec<_> = part.body.solids().filter(|(_, s)| s.open).collect();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].1.name.as_deref(), Some("Extrusion2"));
    // Its wall is named like a solid extrusion's side.
    assert!(part.body.faces().any(|(_, f)| {
        f.name
            .as_deref()
            .is_some_and(|n| n.starts_with("Extrusion2:Side:"))
    }));
    // A later feature does not see the surface: a Join of a second block.
    block(&mut part, (10.0, 30.0), (-5.0, 5.0), 20.0, false);
    healthy(&part);
    assert_eq!(sheets(&part).len(), 1);
}

#[test]
fn revolve_surface_lies_where_the_solid_does() {
    // A rectangle [5, 10] × [0, 10] on XZ turned 90° about Z: the surface's
    // box is the solid's box.
    let build = |output: Output| {
        let mut part = PartDocument::default();
        let i = part
            .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XZ), &opts())
            .unwrap();
        let sk = &mut part.sketches[i].sketch;
        sk.add_rectangle_two_point(Point2::new(5.0, 0.0), Point2::new(10.0, 10.0))
            .unwrap();
        let r = sk.regions();
        let mut rv = RevolveFeature::new(region_profiles(&r, |_| true), AxisRef::Origin(2));
        rv.extent = RevolveExtent::Angle;
        rv.angle = std::f64::consts::FRAC_PI_2;
        rv.output = output;
        part.add_revolve(i, rv).unwrap();
        assert!(part.exact);
        cadcraft3d::kernel::exact::bounds(&part.exact_bodies.0[0]).unwrap()
    };
    let (a, b) = (build(Output::Solid), build(Output::Surface));
    assert!(
        (a.0 - b.0).norm() < 1e-3 && (a.1 - b.1).norm() < 1e-3,
        "{a:?} vs {b:?}"
    );
}

// ------------------------------------------------------ split by planes ---

#[test]
fn split_solid_by_xy_yz_offset_and_angled_planes() {
    // A box centred on z = 0 split by XY.
    let mut part = PartDocument::default();
    block(&mut part, (-20.0, 20.0), (-15.0, 15.0), 20.0, true);
    part.add_split(split(
        SplitMethod::SplitSolid,
        SplitToolRef::Plane(PlaneRef::Origin(OriginPlane::XY)),
    ))
    .unwrap();
    healthy(&part);
    close(vol_of(&part, "Extrusion1"), 12000.0, 1e-9);
    close(vol_of(&part, "Split1#2"), 12000.0, 1e-9);
    assert!(centroid(&part, "Extrusion1").z < 0.0);
    // YZ, an offset work plane (x = 10) and one at 30° about Z.
    for (def, back) in [
        (None, 12000.0),
        (
            Some(WorkPlaneDef::Offset {
                base: PlaneRef::Origin(OriginPlane::YZ),
                distance: 10.0,
            }),
            18000.0,
        ),
        (
            Some(WorkPlaneDef::Angle {
                base: PlaneRef::Origin(OriginPlane::XZ),
                axis: LineRef::Origin(2),
                angle: 30f64.to_radians(),
            }),
            12000.0,
        ),
    ] {
        let mut part = centred();
        let tool = match def {
            None => PlaneRef::Origin(OriginPlane::YZ),
            Some(d) => PlaneRef::Work(part.add_work(WorkFeature::new(WorkDef::Plane(d))).unwrap()),
        };
        part.add_split(split(SplitMethod::SplitSolid, SplitToolRef::Plane(tool)))
            .unwrap();
        healthy(&part);
        close(total(&part), 24000.0, 1e-9);
        close(vol_of(&part, "Extrusion1"), back, 1e-9);
    }
}

// ---------------------------------------------------- split by surfaces ---

#[test]
fn split_solid_by_a_planar_surface() {
    let mut part = centred();
    let s = plane_y5(&mut part);
    part.add_split(split(
        SplitMethod::SplitSolid,
        SplitToolRef::Surface(s.clone()),
    ))
    .unwrap();
    healthy(&part);
    let mut v: Vec<Real> = volumes(&part).iter().map(|x| x.1).collect();
    v.sort_by(Real::total_cmp);
    assert_eq!(v.len(), 2, "{v:?}");
    close(v[0], 8000.0, 1e-9);
    close(v[1], 16000.0, 1e-9);
    // The surface stays; section faces are named.
    assert_eq!(sheets(&part).len(), 1);
    assert!(part.body.find_face("Split1:Cut").is_some());
    assert!(part.body.find_face("Split1#2:Cut").is_some());
    // The new body is listed.
    assert!(
        part.bodies_before(usize::MAX)
            .iter()
            .any(|(n, _)| n == "Split1#2")
    );
}

/// The area of the box's top [−20, 20] × [−15, 15] outside the circle about
/// (0, −60) of radius 70.
fn outside_arc() -> Real {
    let (a, r) = (20.0, 70.0);
    let under = a * (r * r - a * a as Real).sqrt() + r * r * (a / r).asin();
    (15.0 + 60.0) * 2.0 * a - under
}

#[test]
fn split_solid_by_a_curved_surface() {
    let mut part = centred();
    let s = arc_surface(&mut part);
    part.add_split(split(SplitMethod::SplitSolid, SplitToolRef::Surface(s)))
        .unwrap();
    healthy(&part);
    let out = outside_arc() * 20.0;
    let mut v: Vec<Real> = volumes(&part).iter().map(|x| x.1).collect();
    v.sort_by(Real::total_cmp);
    println!("arc surface: {v:?} (want {out}, {})", 24000.0 - out);
    assert_eq!(v.len(), 2);
    // (Curved sections: the mesh at 1e-3 mm is ~1e-4 off; the sum is exact.)
    close(v[0], out, 2e-4);
    close(v[1], 24000.0 - out, 2e-4);
    close(v[0] + v[1], 24000.0, 1e-9);
}

#[test]
fn trim_solid_by_a_surface_default_and_flip() {
    let mut kept = Vec::new();
    for flip in [false, true] {
        let mut part = centred();
        let s = plane_y5(&mut part);
        let mut sp = split(SplitMethod::TrimSolid, SplitToolRef::Surface(s));
        sp.flip = flip;
        // The arrow shows the side that goes (Default) / stays (Flip).
        let tool = cadcraft3d::part::split::resolve_tool(
            sp.tool.as_ref(),
            &part.body,
            &part.work,
            &part.sketches,
        )
        .unwrap();
        let (at, n) = cadcraft3d::part::split::arrow(&tool, Point3::origin()).unwrap();
        assert!((at.y - 5.0).abs() < 1e-6 && n.x.abs() < 1e-9 && n.z.abs() < 1e-9);
        part.add_split(sp).unwrap();
        healthy(&part);
        assert_eq!(volumes(&part).len(), 1);
        let c = centroid(&part, "Extrusion1");
        // Default removes the arrow's side: the kept part lies behind it.
        let behind = (c - at).dot(&n) < 0.0;
        assert_eq!(behind, !flip, "flip {flip}: centroid {c:?}, arrow {n:?}");
        kept.push(total(&part));
        assert!(part.body.find_face("Split1:Cut").is_some());
    }
    let mut v = kept.clone();
    v.sort_by(Real::total_cmp);
    close(v[0], 8000.0, 1e-9);
    close(v[1], 16000.0, 1e-9);
}

#[test]
fn split_face_by_a_surface_then_face_draft() {
    // The surface z = 10 (a line on XZ, ±30 along Y) splits the front face
    // y = −15; the upper piece is drafted 5° about z = 10.
    let mut part = centred();
    let front = face_where(&part, |p| (p.y + 15.0).abs() < 1e-6);
    let s = surface(&mut part, OriginPlane::XZ, 60.0, |sk| {
        vec![sk.add_line(Point2::new(-40.0, 10.0), Point2::new(40.0, 10.0))]
    });
    let mut sp = split(SplitMethod::SplitFace, SplitToolRef::Surface(s));
    sp.faces.push(face_ref(&part, &front, [0.0, -15.0, 5.0]));
    let before = part.body.faces().count();
    part.add_split(sp).unwrap();
    healthy(&part);
    close(total(&part), 24000.0, 1e-9);
    assert_eq!(part.body.faces().count(), before + 1);
    // The piece above z = 10.
    let upper = part
        .body
        .faces()
        .find(|(_, f)| {
            f.name.as_deref().is_some_and(|n| n.contains("~Split1."))
                && f.mesh
                    .as_ref()
                    .is_some_and(|m| m.positions.iter().all(|p| p.z >= 10.0 - 1e-6))
        })
        .and_then(|(_, f)| f.name.clone())
        .expect("no upper piece");
    let wp = part
        .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
            base: PlaneRef::Origin(OriginPlane::XY),
            distance: 10.0,
        })))
        .unwrap();
    let mut d = DraftFeature::new(5f64.to_radians());
    d.draft_type = DraftType::FixedPlane;
    d.plane = Some(PlaneRef::Work(wp));
    d.faces.push(face_ref(&part, &upper, [0.0, -15.0, 15.0]));
    part.add_draft(d).unwrap();
    healthy(&part);
    let wedge = 40.0 * 100.0 * 5f64.to_radians().tan() / 2.0;
    close(total(&part), 24000.0 - wedge, 1e-6);
}

#[test]
fn split_solid_by_a_revolved_surface() {
    // [0, 8] × [−5, 25] on XZ turned fully about Z: a cylinder r = 8 with
    // its end discs outside the box. Core and rest.
    let mut part = centred();
    let i = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XZ), &opts())
        .unwrap();
    let sk = &mut part.sketches[i].sketch;
    sk.add_rectangle_two_point(Point2::new(0.0, -5.0), Point2::new(8.0, 25.0))
        .unwrap();
    let r = sk.regions();
    let mut rv = RevolveFeature::new(region_profiles(&r, |_| true), AxisRef::Origin(2));
    rv.output = Output::Surface;
    let s = part.add_revolve(i, rv).unwrap();
    healthy(&part);
    assert_eq!(sheets(&part).len(), 1);
    part.add_split(split(SplitMethod::SplitSolid, SplitToolRef::Surface(s)))
        .unwrap();
    healthy(&part);
    let core = PI * 64.0 * 20.0;
    let mut v: Vec<Real> = volumes(&part).iter().map(|x| x.1).collect();
    v.sort_by(Real::total_cmp);
    assert_eq!(v.len(), 2, "{v:?}");
    close(v[0], core, 3e-4);
    close(v[1], 24000.0 - core, 1e-4);
    close(v[0] + v[1], 24000.0, 1e-8);
}

#[test]
fn split_solid_by_a_surface_of_two_faces() {
    // An L: y = 5 for x < 5, then x = 5 for y < 5 (one surface, two
    // faces). The corner quadrant x < 5, y < 5 is one part (25 × 20 × 20).
    let mut part = centred();
    let s = surface(&mut part, OriginPlane::XY, 60.0, |sk| {
        let a = sk.add_point(Point2::new(-40.0, 5.0));
        let b = sk.add_point(Point2::new(5.0, 5.0));
        let c = sk.add_point(Point2::new(5.0, -40.0));
        vec![
            sk.add_line_between(a, b).unwrap(),
            sk.add_line_between(b, c).unwrap(),
        ]
    });
    part.add_split(split(SplitMethod::SplitSolid, SplitToolRef::Surface(s)))
        .unwrap();
    healthy(&part);
    let mut v: Vec<Real> = volumes(&part).iter().map(|x| x.1).collect();
    v.sort_by(Real::total_cmp);
    assert_eq!(v.len(), 2, "{v:?}");
    close(v[0], 10000.0, 1e-9);
    close(v[1], 14000.0, 1e-9);
}

#[test]
fn split_by_a_surface_then_fillet() {
    let mut part = centred();
    let s = plane_y5(&mut part);
    part.add_split(split(SplitMethod::SplitSolid, SplitToolRef::Surface(s)))
        .unwrap();
    // The top edge of one section (y = 5, z = 20, along x).
    let edges: Vec<EdgeRef> = edges_where(&part, |p| {
        (p[1] - 5.0).abs() < 1e-6 && (p[2] - 20.0).abs() < 1e-6
    })
    .into_iter()
    .filter(|e| e.faces.iter().any(|f| f == "Split1#2:Cut"))
    .collect();
    assert_eq!(edges.len(), 1, "{edges:?}");
    part.add_blend(BlendFeature::fillet(edges, 2.0)).unwrap();
    healthy(&part);
    close(total(&part), 24000.0 - round_loss(2.0) * 40.0, 1e-4);
}

#[test]
fn surface_refusals_leave_the_part_unchanged() {
    let mut part = centred();
    let s = plane_y5(&mut part);
    let n = part.features.len();
    // A surface that is gone; a solid named as a surface.
    refused(
        part.add_split(split(
            SplitMethod::SplitSolid,
            SplitToolRef::Surface("Srf9".into()),
        )),
        "no longer exists",
    );
    refused(
        part.add_split(split(
            SplitMethod::SplitSolid,
            SplitToolRef::Surface("Extrusion1".into()),
        )),
        "not a surface",
    );
    // A surface beside the body: nothing to split.
    let far = surface(&mut part, OriginPlane::XY, 60.0, |sk| {
        vec![sk.add_line(Point2::new(-40.0, 50.0), Point2::new(40.0, 50.0))]
    });
    let n = n + 1;
    for m in [SplitMethod::SplitSolid, SplitMethod::TrimSolid] {
        refused(
            part.add_split(split(m, SplitToolRef::Surface(far.clone()))),
            "does not split",
        );
    }
    assert_eq!(part.features.len(), n);
    healthy(&part);
    close(total(&part), 24000.0, 1e-9);
    assert_eq!(volumes(&part).len(), 1);
    // Split Face with a surface of two walls (an L along Z): the top face is cut along the L
    // only (spec 5.4a T2), into two pieces — the walls' extended planes would make four.
    let two = surface(&mut part, OriginPlane::XY, 60.0, |sk| {
        let a = sk.add_point(Point2::new(-40.0, 5.0));
        let b = sk.add_point(Point2::new(5.0, 5.0));
        let c = sk.add_point(Point2::new(5.0, -40.0));
        vec![
            sk.add_line_between(a, b).unwrap(),
            sk.add_line_between(b, c).unwrap(),
        ]
    });
    // (the L surface is one more feature)
    let top = face_where(&part, |p| (p.z - 20.0).abs() < 1e-6);
    let mut sp = split(SplitMethod::SplitFace, SplitToolRef::Surface(two));
    sp.faces.push(face_ref(&part, &top, [0.0, 0.0, 20.0]));
    part.add_split(sp).unwrap();
    let _ = s;
    healthy(&part);
    close(total(&part), 24000.0, 1e-9);
    let pieces = part
        .body
        .faces()
        .filter(|(_, f)| {
            f.name
                .as_deref()
                .is_some_and(|x| x.starts_with(&format!("{top}~")))
        })
        .count();
    assert_eq!(pieces, 2, "the top face cut along the L");
}

#[test]
fn split_by_a_surface_save_and_load() {
    let mut part = centred();
    let s = arc_surface(&mut part);
    let mut sp = split(SplitMethod::TrimSolid, SplitToolRef::Surface(s));
    sp.flip = true;
    part.add_split(sp).unwrap();
    healthy(&part);
    let v = total(&part);
    let dir = std::env::temp_dir().join(format!("cadcraft_split_surface_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("ss.3dcadcraft");
    let mut project = Project::new("SS");
    project.add_document("Part1", DocumentData::Part(part.clone()));
    project.save(&f).unwrap();
    let mut back = Project::load(&f).unwrap();
    let id = back.documents()[0].id;
    let Some(DocumentData::Part(p)) = back.document_mut(id).map(|d| &mut d.data) else {
        panic!("no part")
    };
    p.regenerate().unwrap();
    healthy(p);
    close(total(p), v, 1e-9);
    assert_eq!(p.features, part.features);
    assert_eq!(sheets(p).len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

// ------------------------------------------------- split by open curves ---

/// A sketch on XY with a CV spline through `pts`.
fn spline_sketch(part: &mut PartDocument, pts: &[(Real, Real)]) -> String {
    let i = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    let sk = &mut part.sketches[i].sketch;
    let ids = pts
        .iter()
        .map(|p| sk.add_point(Point2::new(p.0, p.1)))
        .collect();
    sk.add_spline(ids).unwrap();
    part.sketches[i].id.clone()
}

/// The owner's session: a loft split by a sketch whose line was replaced by
/// a spline ("a solid is split by one straight line or closed profiles").
#[test]
fn owner_session_split_by_a_spline() {
    let path = std::path::Path::new("tests/data/split_spline_journal.jsonl");
    // The loft before the split (state 17), measured the same way.
    let whole = total(&cadcraft3d::diagnostics::replay::part_at(path, 17).unwrap());
    let part = cadcraft3d::diagnostics::replay::part_at(path, 22).unwrap();
    println!("state 22: {:?} {:?}", part.feature_health, volumes(&part));
    assert!(part.feature_health.is_empty(), "{:?}", part.feature_health);
    assert!(part.exact);
    let v = volumes(&part);
    assert_eq!(v.len(), 2, "{v:?}");
    close(v.iter().map(|x| x.1).sum(), whole, 1e-5);
    // Both parts are real halves of the loft.
    assert!(v.iter().all(|x| x.1 > 0.3 * whole), "{v:?}");
}

#[test]
fn split_solid_by_an_s_spline() {
    // Point-symmetric about the centre of the (centrally symmetric) box:
    // two equal halves.
    let mut part = centred();
    let s = spline_sketch(
        &mut part,
        &[(-30.0, -10.0), (-10.0, -10.0), (10.0, 10.0), (30.0, 10.0)],
    );
    part.add_split(split(SplitMethod::SplitSolid, SplitToolRef::Sketch(s)))
        .unwrap();
    healthy(&part);
    close(vol_of(&part, "Extrusion1"), 12000.0, 1e-6);
    close(vol_of(&part, "Split1#2"), 12000.0, 1e-6);
    // The front (left of the curve's direction, +Y) is the new body.
    assert!(centroid(&part, "Split1#2").y > 1.0);
}

#[test]
fn split_solid_by_a_short_spline_extends_it() {
    // A spline inside the top view; its ends run on along their tangents
    // (±X) to the box walls.
    let mut part = centred();
    let s = spline_sketch(
        &mut part,
        &[(-10.0, -3.0), (-3.0, -3.0), (3.0, 3.0), (10.0, 3.0)],
    );
    part.add_split(split(SplitMethod::SplitSolid, SplitToolRef::Sketch(s)))
        .unwrap();
    healthy(&part);
    close(vol_of(&part, "Extrusion1"), 12000.0, 1e-6);
    close(vol_of(&part, "Split1#2"), 12000.0, 1e-6);
}

#[test]
fn split_solid_by_a_line_and_arc_chain() {
    // (−30, 0) → (0, 0), then a quarter arc about (0, 10) to (10, 10) whose
    // end runs on up along x = 10. Front: above the line, inside the arc,
    // left of x = 10.
    let mut part = centred();
    let i = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    let sk = &mut part.sketches[i].sketch;
    let a = sk.add_point(Point2::new(-30.0, 0.0));
    let b = sk.add_point(Point2::new(0.0, 0.0));
    let c = sk.add_point(Point2::new(0.0, 10.0));
    let d = sk.add_point(Point2::new(10.0, 10.0));
    sk.add_line_between(a, b).unwrap();
    sk.add_arc(c, b, d).unwrap();
    let s = part.sketches[i].id.clone();
    part.add_split(split(SplitMethod::SplitSolid, SplitToolRef::Sketch(s)))
        .unwrap();
    healthy(&part);
    let front = (20.0 * 15.0 + 50.0 + 25.0 * PI) * 20.0;
    println!("chain: {:?} (front {front})", volumes(&part));
    close(vol_of(&part, "Split1#2"), front, 5e-5);
    close(vol_of(&part, "Extrusion1"), 24000.0 - front, 5e-5);
    close(total(&part), 24000.0, 1e-9);
}

#[test]
fn trim_solid_by_a_spline_default_and_flip() {
    for flip in [false, true] {
        let mut part = centred();
        let s = spline_sketch(
            &mut part,
            &[(-30.0, -10.0), (-10.0, -10.0), (10.0, 10.0), (30.0, 10.0)],
        );
        let mut sp = split(SplitMethod::TrimSolid, SplitToolRef::Sketch(s));
        sp.flip = flip;
        let tool = cadcraft3d::part::split::resolve_tool(
            sp.tool.as_ref(),
            &part.body,
            &part.work,
            &part.sketches,
        )
        .unwrap();
        // The arrow at the curve's middle points to its front (+Y here).
        let (_, n) = cadcraft3d::part::split::arrow(&tool, Point3::origin()).unwrap();
        assert!(n.y > 0.5, "{n:?}");
        part.add_split(sp).unwrap();
        healthy(&part);
        close(total(&part), 12000.0, 1e-6);
        let c = centroid(&part, "Extrusion1");
        // Default removes the front (+Y); Flip keeps it.
        assert_eq!(c.y > 0.0, flip, "flip {flip}: {c:?}");
        assert!(
            part.body.find_face("Split1:Cut:0").is_some()
                || part.body.faces().any(|(_, f)| f
                    .name
                    .as_deref()
                    .is_some_and(|n| n.starts_with("Split1:Cut")))
        );
    }
}

#[test]
fn split_by_a_spline_then_fillet() {
    let mut part = centred();
    let s = spline_sketch(
        &mut part,
        &[(-30.0, -10.0), (-10.0, -10.0), (10.0, 10.0), (30.0, 10.0)],
    );
    part.add_split(split(SplitMethod::SplitSolid, SplitToolRef::Sketch(s)))
        .unwrap();
    // The vertical box edge at (20, 15) belongs to the front part.
    let edges: Vec<EdgeRef> = edges_where(&part, |p| {
        (p[0] - 20.0).abs() < 1e-6 && (p[1] - 15.0).abs() < 1e-6
    });
    assert_eq!(edges.len(), 1, "{edges:?}");
    part.add_blend(BlendFeature::fillet(edges, 2.0)).unwrap();
    healthy(&part);
    close(total(&part), 24000.0 - round_loss(2.0) * 20.0, 1e-5);
    close(vol_of(&part, "Extrusion1"), 12000.0, 1e-6);
}

#[test]
fn a_self_crossing_curve_cannot_split_a_solid() {
    let mut part = centred();
    let n = part.features.len();
    // A closed figure eight.
    let i = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    let sk = &mut part.sketches[i].sketch;
    let pts: Vec<_> = [
        (14.0, 0.0),
        (10.0, -8.0),
        (-10.0, 8.0),
        (-14.0, 0.0),
        (-10.0, -8.0),
        (10.0, 8.0),
    ]
    .iter()
    .map(|p| sk.add_point(Point2::new(p.0, p.1)))
    .collect();
    let mut ids: Vec<_> = pts.clone();
    ids.push(pts[0]);
    sk.add_spline(ids).unwrap();
    let s = part.sketches[i].id.clone();
    refused(
        part.add_split(split(
            SplitMethod::SplitSolid,
            SplitToolRef::Sketch(s.clone()),
        )),
        "crosses itself",
    );
    // An open curve turning back across its own extension.
    let s2 = spline_sketch(
        &mut part,
        &[
            (-10.0, 0.0),
            (10.0, 0.0),
            (10.0, 10.0),
            (0.0, 10.0),
            (0.0, -10.0),
        ],
    );
    refused(
        part.add_split(split(SplitMethod::TrimSolid, SplitToolRef::Sketch(s2))),
        "crosses itself",
    );
    assert_eq!(part.features.len(), n);
    healthy(&part);
    close(total(&part), 24000.0, 1e-9);
}
