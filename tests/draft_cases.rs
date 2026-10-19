//! Face Draft options beyond One Way (`docs/spec/2.8-draft.md`): Fixed
//! Plane Symmetric / Asymmetric, Parting Line (plane and sketch), Move
//! Parting Line (Top / Bottom / Both), Automatic Blending, refusals, edit,
//! save / load and cross-feature cases. Every volume is analytic.

mod common;

use cadcraft3d::part::{
    DraftExtent, LineRef, PartingSide, PartingTool, PullRef, WorkDef, WorkFeature, WorkPlaneDef,
};
use common::*;

/// A work plane parallel to XY at height `z`.
fn level(part: &mut PartDocument, z: Real) -> PlaneRef {
    let id = part
        .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
            base: PlaneRef::Origin(OriginPlane::XY),
            distance: z,
        })))
        .unwrap();
    PlaneRef::Work(id)
}

/// The plate's sides drafted from z = `h`: `a` above, `b` below (the
/// two frusta of a W × D section).
fn two_frusta(w: Real, d: Real, top: Real, h: Real, a: Real, b: Real) -> Real {
    let (ka, kb) = (k(a), k(b));
    simpson(&|z| (w - 2.0 * z * ka) * (d - 2.0 * z * ka), 0.0, top - h)
        + simpson(&|z| (w - 2.0 * z * kb) * (d - 2.0 * z * kb), 0.0, h)
}

fn up() -> PullRef {
    PullRef::Plane(PlaneRef::Origin(OriginPlane::XY))
}

/// The plate 40 × 30 × 20 with its sides drafted by `set`.
fn drafted(set: impl FnOnce(&mut PartDocument, &mut DraftFeature)) -> Result<PartDocument, String> {
    let mut p = plate(40.0, 30.0, 20.0);
    let s = sides(&p, 40.0, 30.0, 10.0);
    let mut d = DraftFeature::new(5f64.to_radians());
    for (n, at) in &s {
        d.faces.push(face_ref(&p, n, *at));
    }
    set(&mut p, &mut d);
    p.add_draft(d).map_err(|e| e.to_string())?;
    Ok(p)
}

// -------------------------------------------------- Fixed Plane, two-sided ---

#[test]
fn fixed_plane_symmetric_and_asymmetric() {
    for (h, a, b, ext) in [
        (10.0, 5.0, 5.0, DraftExtent::Symmetric),
        (10.0, 5.0, 3.0, DraftExtent::Asymmetric),
        (5.0, 4.0, 4.0, DraftExtent::Symmetric),
        (15.0, 2.0, 6.0, DraftExtent::Asymmetric),
        (10.0, -3.0, -3.0, DraftExtent::Symmetric),
        (12.0, 3.0, -2.0, DraftExtent::Asymmetric),
    ] {
        let p = drafted(|p, d| {
            d.draft_type = DraftType::FixedPlane;
            d.plane = Some(level(p, h));
            d.extent = ext;
            d.angle = Real::to_radians(a);
            d.angle2 = Real::to_radians(b);
        })
        .unwrap();
        check(&p, two_frusta(40.0, 30.0, 20.0, h, a, b), 1e-6);
        // Each side in two pieces, both keep its name.
        let front = face_where(&p, |q| q.y < 2.0 && q.z > h - 1e-6);
        let n = p
            .body
            .faces()
            .filter(|(_, f)| f.name.as_deref() == Some(front.as_str()))
            .count();
        assert_eq!(n, 2, "{front}");
    }
}

/// One Way keeps the old behaviour (whole faces, one direction).
#[test]
fn fixed_plane_one_way_unchanged() {
    let p = drafted(|_, d| {
        d.draft_type = DraftType::FixedPlane;
        d.plane = Some(PlaneRef::Origin(OriginPlane::XY));
    })
    .unwrap();
    check(&p, two_frusta(40.0, 30.0, 20.0, 0.0, 5.0, 0.0), 1e-6);
}

/// A cylinder R15 × 50 split at z = 20: two cones meeting at R15.
#[test]
fn symmetric_draft_of_a_cylinder() {
    let mut p = PartDocument::default();
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    p.sketches[i]
        .sketch
        .add_circle(Point2::new(0.0, 0.0), 15.0)
        .unwrap();
    let r = p.sketches[i].sketch.regions();
    p.add_extrude(i, ExtrudeFeature::new(region_profiles(&r, |_| true), 50.0))
        .unwrap();
    let wall = face_where(&p, |q| (q.x * q.x + q.y * q.y - 225.0).abs() < 1e-3);
    let mut d = DraftFeature::new(4f64.to_radians());
    d.draft_type = DraftType::FixedPlane;
    d.plane = Some(level(&mut p, 20.0));
    d.extent = DraftExtent::Symmetric;
    d.faces.push(face_ref(&p, &wall, [15.0, 0.0, 25.0]));
    p.add_draft(d).unwrap();
    let kk = k(4.0);
    let frustum = |h: Real| {
        let r = 15.0 - h * kk;
        PI * h / 3.0 * (225.0 + 15.0 * r + r * r)
    };
    check(&p, frustum(30.0) + frustum(20.0), 1e-3);
}

/// The L's two long outer walls, 3° both ways about z = 5.
#[test]
fn symmetric_draft_of_an_l_prism() {
    let mut p = PartDocument::default();
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    let pts = [
        (0.0, 0.0),
        (30.0, 0.0),
        (30.0, 10.0),
        (10.0, 10.0),
        (10.0, 30.0),
        (0.0, 30.0),
    ];
    {
        let sk = &mut p.sketches[i].sketch;
        let ids: Vec<_> = pts
            .iter()
            .map(|(x, y)| sk.add_point(Point2::new(*x, *y)))
            .collect();
        for j in 0..ids.len() {
            sk.add_line_between(ids[j], ids[(j + 1) % ids.len()])
                .unwrap();
        }
    }
    let r = p.sketches[i].sketch.regions();
    p.add_extrude(i, ExtrudeFeature::new(region_profiles(&r, |_| true), 10.0))
        .unwrap();
    let a = face_where(&p, |q| q.y.abs() < 1e-6);
    let b = face_where(&p, |q| q.x.abs() < 1e-6);
    let mut d = DraftFeature::new(3f64.to_radians());
    d.draft_type = DraftType::FixedPlane;
    d.plane = Some(level(&mut p, 5.0));
    d.extent = DraftExtent::Symmetric;
    d.faces.push(face_ref(&p, &a, [15.0, 0.0, 5.0]));
    d.faces.push(face_ref(&p, &b, [0.0, 15.0, 5.0]));
    p.add_draft(d).unwrap();
    let kk = k(3.0);
    let lost = 2.0 * (2.0 * 30.0 * 0.5 * 5.0 * 5.0 * kk - kk * kk * 125.0 / 3.0);
    check(&p, 500.0 * 10.0 - lost, 1e-6);
}

// ------------------------------------------------------------ Parting Line ---

#[test]
fn parting_line_plane_and_pull_directions() {
    // Pull from the XY plane, from the Z axis, from a vertical edge, and
    // flipped (the sides swap: Draft Angle goes below).
    for (pull, flip, h, a, b, ext) in [
        (up(), false, 12.0, 3.0, 3.0, DraftExtent::Symmetric),
        (
            PullRef::Line(LineRef::Origin(2)),
            false,
            8.0,
            2.0,
            5.0,
            DraftExtent::Asymmetric,
        ),
        (up(), true, 8.0, 2.0, 5.0, DraftExtent::Asymmetric),
        (up(), false, 10.0, 4.0, 4.0, DraftExtent::Symmetric),
        (
            PullRef::Line(LineRef::Origin(2)),
            false,
            6.0,
            1.0,
            1.0,
            DraftExtent::Symmetric,
        ),
    ] {
        let p = drafted(|p, d| {
            d.set_type(DraftType::PartingLine);
            d.pull = Some(pull.clone());
            d.flip = flip;
            d.parting = Some(PartingTool::Plane(level(p, h)));
            d.extent = ext;
            d.angle = Real::to_radians(a);
            d.angle2 = Real::to_radians(b);
        })
        .unwrap();
        // Flipped: "above" is below z = h.
        let expect = if flip {
            two_frusta(40.0, 30.0, 20.0, h, b, a)
        } else {
            two_frusta(40.0, 30.0, 20.0, h, a, b)
        };
        check(&p, expect, 1e-6);
    }
}

/// A parting line from a sketch on XZ: a line at z = 8 (swept along the
/// sketch normal it is the plane z = 8).
#[test]
fn parting_line_from_a_sketch() {
    let p = drafted(|p, d| {
        let j = p
            .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XZ), &opts())
            .unwrap();
        let sk = &mut p.sketches[j].sketch;
        let a = sk.add_point(Point2::new(-5.0, 8.0));
        let b = sk.add_point(Point2::new(45.0, 8.0));
        sk.add_line_between(a, b).unwrap();
        d.set_type(DraftType::PartingLine);
        d.pull = Some(up());
        d.parting = Some(PartingTool::Sketch(p.sketches[j].id.clone()));
        d.extent = DraftExtent::Asymmetric;
        d.angle = 2f64.to_radians();
        d.angle2 = 6f64.to_radians();
    })
    .unwrap();
    check(&p, two_frusta(40.0, 30.0, 20.0, 8.0, 2.0, 6.0), 1e-6);
    // The sketch is consumed by the draft.
    let j = p.sketches.len() - 1;
    assert_eq!(p.consumers(&p.sketches[j].id).len(), 1);
}

// --------------------------------------------------------- Move Parting Line ---

fn move_parting(side: PartingSide, a: Real, b: Real) -> Result<PartDocument, String> {
    drafted(|p, d| {
        d.set_type(DraftType::PartingLine);
        d.pull = Some(up());
        d.parting = Some(PartingTool::Plane(level(p, 10.0)));
        d.move_parting = true;
        d.move_side = side;
        d.extent = if a == b {
            DraftExtent::Symmetric
        } else {
            DraftExtent::Asymmetric
        };
        d.angle = a.to_radians();
        d.angle2 = b.to_radians();
        let n = d.select_boundary(&p.body, &p.work, None).unwrap();
        let want = if side == PartingSide::Both { 8 } else { 4 };
        assert_eq!(n, want, "{side:?}");
    })
}

/// Section of the plate widened by `k·t` on each side.
fn wide(kk: Real, t: Real) -> Real {
    (40.0 + 2.0 * t * kk) * (30.0 + 2.0 * t * kk)
}

#[test]
fn move_parting_line_top_bottom_both() {
    let kk = k(3.0);
    // Top: the upper half pivots about its top edges and overhangs.
    let p = move_parting(PartingSide::Top, 3.0, 3.0).unwrap();
    check(
        &p,
        12_000.0 + simpson(&|z| wide(kk, 20.0 - z), 10.0, 20.0),
        1e-6,
    );
    // Bottom: the lower half about its bottom edges (leaning in going down).
    let p = move_parting(PartingSide::Bottom, 3.0, 3.0).unwrap();
    check(&p, 12_000.0 + simpson(&|z| wide(kk, z), 0.0, 10.0), 1e-6);
    // Both: each half about its own edges; Draft Angle 2 below.
    let p = move_parting(PartingSide::Both, 3.0, 2.0).unwrap();
    let k2 = k(2.0);
    check(
        &p,
        simpson(&|z| wide(kk, 20.0 - z), 10.0, 20.0) + simpson(&|z| wide(k2, z), 0.0, 10.0),
        1e-6,
    );
}

// -------------------------------------------------------- Automatic Blending ---

/// Vertical edges rounded r5; only the four flat sides picked (chain off):
/// Automatic Blending drafts the rounds too, a tapered rounded rectangle.
#[test]
fn automatic_blending_drafts_the_rounds() {
    let mut p = plate(40.0, 30.0, 20.0);
    let e = vertical(&p, 40.0, 30.0, 20.0);
    p.add_blend(BlendFeature::fillet(e, 5.0)).unwrap();
    let s = sides(&p, 40.0, 30.0, 10.0);
    let mut d = DraftFeature::new(5f64.to_radians());
    d.draft_type = DraftType::FixedPlane;
    d.plane = Some(PlaneRef::Origin(OriginPlane::XY));
    d.chain = false;
    for (n, at) in &s {
        d.faces.push(face_ref(&p, n, *at));
    }
    assert!(d.blending);
    let (before, v0) = (p.features.len(), volume(&p));
    let mut off = d.clone();
    off.blending = false;
    // Without blending the rounds stay upright: the kernel refuses, the
    // part is unchanged.
    let r = p.clone().add_draft(off);
    assert!(r.is_err(), "{r:?}");
    p.add_draft(d).unwrap();
    let kk = k(5.0);
    let area = |z: Real| {
        let d = z * kk;
        (40.0 - 2.0 * d) * (30.0 - 2.0 * d) - (4.0 - PI) * (5.0 - d) * (5.0 - d)
    };
    check(&p, simpson(&area, 0.0, 20.0), 1e-3);
    assert_eq!(p.features.len(), before + 1);
    assert!(v0 > volume(&p));
}

/// Symmetric with blending: the rounds become two cones meeting at z = 10.
#[test]
fn automatic_blending_symmetric() {
    let mut p = plate(40.0, 30.0, 20.0);
    let e = vertical(&p, 40.0, 30.0, 20.0);
    p.add_blend(BlendFeature::fillet(e, 5.0)).unwrap();
    let s = sides(&p, 40.0, 30.0, 10.0);
    let mut d = DraftFeature::new(4f64.to_radians());
    d.draft_type = DraftType::FixedPlane;
    d.plane = Some(level(&mut p, 10.0));
    d.extent = DraftExtent::Symmetric;
    d.chain = false;
    for (n, at) in &s {
        d.faces.push(face_ref(&p, n, *at));
    }
    p.add_draft(d).unwrap();
    let kk = k(4.0);
    let area = |t: Real| {
        let d = t * kk;
        (40.0 - 2.0 * d) * (30.0 - 2.0 * d) - (4.0 - PI) * (5.0 - d) * (5.0 - d)
    };
    check(&p, 2.0 * simpson(&area, 0.0, 10.0), 1e-3);
}

// ---------------------------------------------------------------- refusals ---

#[test]
fn refusals_leave_the_part_unchanged() {
    let base = plate(40.0, 30.0, 20.0);
    let v0 = volume(&base);
    let n0 = base.features.len();
    let s = sides(&base, 40.0, 30.0, 10.0);
    let faces: Vec<FaceRef> = s.iter().map(|(n, at)| face_ref(&base, n, *at)).collect();
    let try_def = |set: &dyn Fn(&mut PartDocument, &mut DraftFeature)| {
        let mut p = base.clone();
        let mut d = DraftFeature::new(3f64.to_radians());
        d.faces = faces.clone();
        set(&mut p, &mut d);
        let n = p.features.len();
        let r = p.add_draft(d);
        assert_eq!(p.features.len(), n);
        assert!((volume(&p) - v0).abs() < 1e-6);
        r.map_err(|e| e.to_string())
    };
    // A parting plane not square to the pull (the pull along X).
    refused(
        try_def(&|p, d| {
            d.set_type(DraftType::PartingLine);
            d.pull = Some(PullRef::Line(LineRef::Origin(0)));
            d.parting = Some(PartingTool::Plane(level(p, 10.0)));
        }),
        "not square to the pull direction",
    );
    // A slanted parting line in a sketch.
    refused(
        try_def(&|p, d| {
            let j = p
                .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XZ), &opts())
                .unwrap();
            let sk = &mut p.sketches[j].sketch;
            let a = sk.add_point(Point2::new(-5.0, 4.0));
            let b = sk.add_point(Point2::new(45.0, 12.0));
            sk.add_line_between(a, b).unwrap();
            d.set_type(DraftType::PartingLine);
            d.pull = Some(up());
            d.parting = Some(PartingTool::Sketch(p.sketches[j].id.clone()));
        }),
        "not at one height",
    );
    // Move Parting Line without static edges.
    refused(
        try_def(&|p, d| {
            d.set_type(DraftType::PartingLine);
            d.pull = Some(up());
            d.parting = Some(PartingTool::Plane(level(p, 10.0)));
            d.move_parting = true;
        }),
        "static edges",
    );
    // No parting tool.
    refused(
        try_def(&|_, d| {
            d.set_type(DraftType::PartingLine);
            d.pull = Some(up());
        }),
        "parting tool",
    );
    // Draft Angle 2 out of range.
    refused(
        try_def(&|p, d| {
            d.draft_type = DraftType::FixedPlane;
            d.plane = Some(level(p, 10.0));
            d.extent = DraftExtent::Asymmetric;
            d.angle2 = 95f64.to_radians();
        }),
        "angle 2",
    );
    assert_eq!(base.features.len(), n0);
}

// ------------------------------------------- edit, save / load, cross tests ---

#[test]
fn edit_symmetric_to_asymmetric_and_save_load() {
    let mut p = drafted(|p, d| {
        d.draft_type = DraftType::FixedPlane;
        d.plane = Some(level(p, 10.0));
        d.extent = DraftExtent::Symmetric;
    })
    .unwrap();
    let k_draft = p.features.len() - 1;
    let Some(Feature::Draft(mut d)) = p.features.get(k_draft).cloned() else {
        panic!("not a draft");
    };
    d.extent = DraftExtent::Asymmetric;
    d.angle2 = 2f64.to_radians();
    p.replace_feature(k_draft, Feature::Draft(d)).unwrap();
    check(&p, two_frusta(40.0, 30.0, 20.0, 10.0, 5.0, 2.0), 1e-6);
    let v = volume(&p);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("draft.3dcadcraft");
    let mut project = Project::new("Draft");
    project.add_document("Part1", DocumentData::Part(p.clone()));
    project.save(&path).unwrap();
    let mut back = Project::load(&path).unwrap();
    let id = back.documents()[0].id;
    let Some(DocumentData::Part(q)) = back.document_mut(id).map(|d| &mut d.data) else {
        panic!("part");
    };
    q.regenerate().unwrap();
    assert_eq!(q.features, p.features);
    check(q, v, 1e-9);
}

/// A shell open at the top, then its outer sides drafted both ways about
/// z = 10: the cavity stays.
#[test]
fn shell_then_symmetric_draft() {
    let mut p = plate(40.0, 30.0, 20.0);
    let t = top(&p, 20.0);
    shell(
        &mut p,
        &[(&t, [20.0, 15.0, 20.0])],
        2.0,
        ShellDirection::Inside,
    );
    let s = sides(&plate(40.0, 30.0, 20.0), 40.0, 30.0, 10.0);
    let mut d = DraftFeature::new(5f64.to_radians());
    d.draft_type = DraftType::FixedPlane;
    d.plane = Some(level(&mut p, 10.0));
    d.extent = DraftExtent::Symmetric;
    for (n, at) in &s {
        d.faces.push(face_ref(&p, n, *at));
    }
    p.add_draft(d).unwrap();
    let cavity = 36.0 * 26.0 * 18.0;
    check(
        &p,
        two_frusta(40.0, 30.0, 20.0, 10.0, 5.0, 5.0) - cavity,
        1e-6,
    );
}

/// Symmetric draft, then the top outline rounded r1 (a 95° edge).
#[test]
fn symmetric_draft_then_fillet() {
    let mut p = drafted(|p, d| {
        d.draft_type = DraftType::FixedPlane;
        d.plane = Some(level(p, 10.0));
        d.extent = DraftExtent::Symmetric;
    })
    .unwrap();
    let e = edges_where(&p, |h| (h[2] - 20.0).abs() < 1e-6);
    assert_eq!(e.len(), 4);
    p.add_blend(BlendFeature::fillet(e, 1.0)).unwrap();
    let phi = 95f64.to_radians();
    let lost = 1.0 / (phi / 2.0).tan() - (PI - phi) / 2.0;
    let kk = k(5.0);
    let (a, b) = (40.0 - 20.0 * kk, 30.0 - 20.0 * kk);
    // Corner patches ignored (~r³).
    check(
        &p,
        two_frusta(40.0, 30.0, 20.0, 10.0, 5.0, 5.0) - lost * 2.0 * (a + b),
        2e-4,
    );
}

/// Symmetric draft, then a shell open at the top: valid, lighter than the
/// drafted block by about the cavity.
#[test]
fn symmetric_draft_then_shell() {
    let mut p = drafted(|p, d| {
        d.draft_type = DraftType::FixedPlane;
        d.plane = Some(level(p, 10.0));
        d.extent = DraftExtent::Symmetric;
    })
    .unwrap();
    let full = volume(&p);
    let t = face_where(&p, |q| (q.z - 20.0).abs() < 1e-6);
    let kk = k(5.0);
    shell(
        &mut p,
        &[(&t, [20.0, 15.0, 20.0])],
        1.0,
        ShellDirection::Inside,
    );
    // The cavity: the same double frustum 2 mm narrower, from z = 1 up
    // (walls offset square to the faces: 1/cos5° wider in plan).
    let w = 1.0 / 5f64.to_radians().cos();
    let section = |z: Real| {
        let s = (z - 10.0).abs() * kk;
        (40.0 - 2.0 * w - 2.0 * s) * (30.0 - 2.0 * w - 2.0 * s)
    };
    let cav = simpson(&section, 1.0, 10.0) + simpson(&section, 10.0, 20.0);
    check(&p, full - cav, 1e-6);
}
