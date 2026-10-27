//! Multi-body parts end to end: a plate split into two bodies, then each
//! body worked on its own — Shell, sketches on one body's faces, Join / Cut
//! limited to one body (Inventor's **Solids**), Fillet on both, Combine,
//! Move Bodies, Delete Face — editing the Split upstream, suppressing it,
//! save / load, and the owner's box (base + lid) from split to an exploded
//! lid. Every step checks the volume of each body, not only the total.

mod common;
use common::*;

use cadcraft3d::part::body_op::{DeleteFaceOp, DeleteSelect, MoveBodiesOp, MoveStep};
use cadcraft3d::part::{
    BodyOp, BodyOpFeature, CombineFeature, SplitFeature, SplitMethod, SplitToolRef, WorkDef,
    WorkFeature, WorkPlaneDef,
};

// ------------------------------------------------------------ helpers ---

const W: Real = 100.0;
const D: Real = 30.0;
const H: Real = 20.0;
/// The left body (x < 50) keeps the plate's name; the right one is new.
const A: &str = "Extrusion1";
const B: &str = "Split1#2";

/// The plate 100 × 30 × 20 split at x = `at` (a work plane offset from YZ).
fn split_plate_at(at: Real) -> PartDocument {
    let mut p = plate(W, D, H);
    let wp = p
        .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
            base: PlaneRef::Origin(OriginPlane::YZ),
            distance: at,
        })))
        .unwrap();
    let mut s = SplitFeature::new(SplitMethod::SplitSolid);
    s.tool = Some(SplitToolRef::Plane(PlaneRef::Work(wp)));
    assert_eq!(p.add_split(s).unwrap(), "Split1");
    assert_eq!(names(&p), vec![A.to_owned(), B.to_owned()]);
    p
}

fn split_plate() -> PartDocument {
    split_plate_at(50.0)
}

fn names(p: &PartDocument) -> Vec<String> {
    p.exact_bodies
        .0
        .iter()
        .filter_map(|s| s.name.clone())
        .collect()
}

/// Volume of body `name` (exact kernel), 0 when it is a surface.
fn vol(p: &PartDocument, name: &str) -> Real {
    p.exact_bodies
        .0
        .iter()
        .find(|b| b.name.as_deref() == Some(name))
        .map(|b| {
            if b.is_open() {
                0.0
            } else {
                b.measure_full().map(|m| m.volume).unwrap_or(0.0)
            }
        })
        .unwrap_or_else(|| panic!("no body {name}: {:?}", names(p)))
}

fn near(v: Real, want: Real, what: &str) {
    assert!(
        (v - want).abs() <= 2e-5 * want.abs().max(1.0),
        "{what}: {v:.4} vs {want:.4}"
    );
}

/// Exact kernel, nothing sick, every closed body valid.
fn healthy(p: &PartDocument) {
    assert!(p.exact, "built with the faceted kernel");
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    for (s, sol) in p.body.solids() {
        if !sol.open {
            p.body.validate_solid(s).unwrap();
        }
    }
}

/// The box of body `name`'s display points.
fn bbox(p: &PartDocument, name: &str) -> (Point3, Point3) {
    let mut lo = Point3::new(Real::INFINITY, Real::INFINITY, Real::INFINITY);
    let mut hi = -lo;
    for f in cadcraft3d::part::select::faces_of_body(&p.body, name) {
        if let Some(m) = p.body.face(f).and_then(|f| f.mesh.as_ref()) {
            for q in &m.positions {
                lo = lo.inf(q);
                hi = hi.sup(q);
            }
        }
    }
    (lo, hi)
}

/// The face of body `body` all of whose display points satisfy `on`, as a
/// reference with the hint `at`.
fn body_face(p: &PartDocument, body: &str, on: impl Fn(&Point3) -> bool, at: [Real; 3]) -> FaceRef {
    let fid = cadcraft3d::part::select::faces_of_body(&p.body, body)
        .into_iter()
        .find(|&f| {
            p.body
                .face(f)
                .and_then(|f| f.mesh.as_ref())
                .is_some_and(|m| m.positions.iter().all(&on))
        })
        .unwrap_or_else(|| panic!("no such face on {body}"));
    cadcraft3d::part::shell::face_ref(&p.body, fid, Some(Point3::from(at))).expect("face ref")
}

/// Shell body `body` 2 mm from its top (z = 20) at `at`.
fn shell_top(p: &mut PartDocument, body: &str, at: [Real; 3]) {
    let mut sh = ShellFeature::new(2.0);
    sh.remove
        .push(body_face(p, body, |q| (q.z - H).abs() < 1e-6, at));
    sh.solids = vec![body.to_owned()];
    p.add_shell(sh)
        .unwrap_or_else(|e| panic!("shell {body}: {e}"));
}

/// A w × d box shelled 2 mm from the top: 2 walls each way, floor 2.
fn shelled(w: Real, d: Real, h: Real) -> Real {
    w * d * h - (w - 4.0) * (d - 4.0) * (h - 2.0)
}

fn add_move(p: &mut PartDocument, body: &str, x: Real, y: Real, z: Real) -> String {
    let mut m = MoveBodiesOp::new();
    m.bodies = vec![body.to_owned()];
    m.steps = vec![MoveStep::FreeDrag { x, y, z }];
    p.add_body_op(BodyOpFeature::new(BodyOp::MoveBodies(m)))
        .unwrap_or_else(|e| panic!("move {body}: {e}"))
}

/// A rectangle y 5..25, z `z0`..`z0 + 10` on the plate's end face x = 100
/// (body B), extruded 10 mm out of it and joined to `solids`.
fn boss_on_end(p: &mut PartDocument, z0: Real, solids: &[&str]) -> String {
    let end = face_where(p, |q| (q.x - W).abs() < 1e-6);
    let j = sketch_on(p, &end, |sk, at| {
        sk.add_rectangle_two_point(at(W, 5.0, z0), at(W, 25.0, z0 + 10.0))
            .unwrap();
    });
    // The rectangle only (the face's own outline is projected too).
    let r = smallest(&p.sketches[j].sketch.regions());
    let mut ex = ExtrudeFeature::new(r, 10.0);
    ex.operation = Operation::Join;
    ex.solids = solids.iter().map(|s| (*s).to_owned()).collect();
    let id = p.add_extrude(j, ex).unwrap();
    // Out of the face, not into the body.
    if bbox(p, B).1.x < W + 9.0 {
        let k = p.features.iter().position(|f| f.id() == id).unwrap();
        let Feature::Extrude(mut e) = p.features[k].clone() else {
            panic!()
        };
        e.direction = ExtentDirection::Flipped;
        p.replace_feature(k, Feature::Extrude(e)).unwrap();
    }
    id
}

/// Circles of radius `r` at the (x, y) centres on the XY plane, cut through
/// everything on both sides, limited to `solids` (empty: all).
fn holes(p: &mut PartDocument, centres: &[(Real, Real)], r: Real, solids: &[&str]) -> String {
    let j = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    for &(x, y) in centres {
        p.sketches[j]
            .sketch
            .add_circle(Point2::new(x, y), r)
            .unwrap();
    }
    let reg = p.sketches[j].sketch.regions();
    let mut ex = ExtrudeFeature::new(region_profiles(&reg, |_| true), 1.0);
    ex.operation = Operation::Cut;
    ex.extent = Extent::ThroughAll;
    ex.direction = ExtentDirection::Symmetric;
    ex.solids = solids.iter().map(|s| (*s).to_owned()).collect();
    p.add_extrude(j, ex).unwrap()
}

/// The profile of the smallest region.
fn smallest(regions: &[cadcraft3d::sketch::Region]) -> Vec<ProfileRef> {
    let r = regions
        .iter()
        .min_by(|a, b| a.area().total_cmp(&b.area()))
        .expect("regions");
    vec![ProfileRef::of(r, r.interior_point())]
}

fn combine(base: &str, tools: &[&str]) -> CombineFeature {
    let mut c = CombineFeature::new();
    c.base = base.into();
    c.tools = tools.iter().map(|t| t.to_string()).collect();
    c.operation = Operation::Join;
    c
}

fn save_load(p: &PartDocument, tag: &str) -> PartDocument {
    let dir = std::env::temp_dir().join(format!("cadcraft_multi_{tag}_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("m.3dcadcraft");
    let mut project = Project::new("Multi");
    project.add_document("Part1", DocumentData::Part(p.clone()));
    project.save(&f).unwrap();
    let back = Project::load(&f).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let mut q = back
        .documents()
        .iter()
        .find_map(|d| match &d.data {
            DocumentData::Part(p) => Some(p.clone()),
            _ => None,
        })
        .expect("a part");
    q.regenerate().unwrap();
    q
}

const HALF: Real = 50.0 * D * H;

// -------------------------------------------------------------- tests ---

#[test]
fn shell_each_half_on_its_own() {
    let mut p = split_plate();
    shell_top(&mut p, A, [25.0, 15.0, H]);
    healthy(&p);
    near(vol(&p, A), shelled(50.0, D, H), "A shelled");
    near(vol(&p, B), HALF, "B untouched");
    shell_top(&mut p, B, [75.0, 15.0, H]);
    healthy(&p);
    near(vol(&p, A), shelled(50.0, D, H), "A");
    near(vol(&p, B), shelled(50.0, D, H), "B shelled");
    assert_eq!(names(&p).len(), 2);
}

#[test]
fn a_boss_on_one_body_joins_only_it() {
    let mut p = split_plate();
    boss_on_end(&mut p, 5.0, &[B]);
    healthy(&p);
    near(vol(&p, A), HALF, "A");
    near(vol(&p, B), HALF + 20.0 * 10.0 * 10.0, "B with the boss");
    let (_, hi) = bbox(&p, B);
    assert!((hi.x - (W + 10.0)).abs() < 1e-6, "{hi:?}");
    assert_eq!(names(&p).len(), 2);
}

#[test]
fn a_cut_across_both_bodies_limited_to_one() {
    // A hole r 5 on the split plane: only A loses its half.
    let mut p = split_plate();
    holes(&mut p, &[(50.0, 15.0)], 5.0, &[A]);
    healthy(&p);
    let half_hole = PI * 25.0 * H / 2.0;
    near(vol(&p, A), HALF - half_hole, "A cut");
    near(vol(&p, B), HALF, "B untouched");
}

#[test]
fn a_cut_of_all_bodies_by_default() {
    let mut p = split_plate();
    holes(
        &mut p,
        &[(25.0, 15.0), (75.0, 15.0), (50.0, 15.0)],
        5.0,
        &[],
    );
    healthy(&p);
    let hole = PI * 25.0 * H;
    near(vol(&p, A), HALF - 1.5 * hole, "A");
    near(vol(&p, B), HALF - 1.5 * hole, "B");
}

#[test]
fn fillet_edges_of_both_bodies_in_one_feature() {
    let mut p = split_plate();
    // The outer vertical edges at y = 0: x = 0 (A) and x = 100 (B).
    let edges = edges_where(&p, |q| {
        q[1].abs() < 1e-6 && (q[0].abs() < 1e-6 || (q[0] - W).abs() < 1e-6) && q[2] > 1e-6
    });
    assert_eq!(edges.len(), 2, "{edges:?}");
    p.add_blend(BlendFeature::fillet(edges, 3.0)).unwrap();
    healthy(&p);
    let round = round_loss(3.0) * H;
    near(vol(&p, A), HALF - round, "A");
    near(vol(&p, B), HALF - round, "B");
}

#[test]
fn combine_the_shelled_halves_into_one() {
    let mut p = split_plate();
    shell_top(&mut p, A, [25.0, 15.0, H]);
    shell_top(&mut p, B, [75.0, 15.0, H]);
    p.add_combine(combine(A, &[B])).unwrap();
    healthy(&p);
    assert_eq!(names(&p), vec![A.to_owned()]);
    // The two walls at x = 50 touch over their whole face: one 4 mm wall.
    near(vol(&p, A), 2.0 * shelled(50.0, D, H), "joined");
}

#[test]
fn a_moved_body_keeps_its_sketches_and_features() {
    let mut p = split_plate();
    add_move(&mut p, B, 0.0, 0.0, 40.0);
    healthy(&p);
    let (lo, hi) = bbox(&p, B);
    assert!((lo.z - 40.0).abs() < 1e-6 && (hi.z - 60.0).abs() < 1e-6);
    // A boss sketched on the moved end face lands on it.
    boss_on_end(&mut p, 45.0, &[B]);
    healthy(&p);
    near(vol(&p, B), HALF + 2000.0, "B with the boss");
    near(vol(&p, A), HALF, "A");
    let (_, hi) = bbox(&p, B);
    assert!((hi.x - (W + 10.0)).abs() < 1e-6, "{hi:?}");
    // A hole of every body still meets both where they now are.
    holes(&mut p, &[(25.0, 15.0), (75.0, 15.0)], 4.0, &[]);
    healthy(&p);
    let hole = PI * 16.0 * H;
    near(vol(&p, A), HALF - hole, "A drilled");
    near(vol(&p, B), HALF + 2000.0 - hole, "B drilled");
}

#[test]
fn delete_a_face_of_one_body() {
    let mut p = split_plate();
    let mut d = DeleteFaceOp::new();
    d.faces.push(body_face(
        &p,
        B,
        |q| (q.z - H).abs() < 1e-6,
        [75.0, 15.0, H],
    ));
    p.add_body_op(BodyOpFeature::new(BodyOp::DeleteFace(d)))
        .unwrap();
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    // B opens into a surface; A stays a solid.
    let open = |n: &str| {
        p.exact_bodies
            .0
            .iter()
            .find(|b| b.name.as_deref() == Some(n))
            .map(|b| b.is_open())
    };
    assert_eq!((open(A), open(B)), (Some(false), Some(true)));
    near(vol(&p, A), HALF, "A");
    // Lump of A, a body of one shell: refused (spec 2.14), nothing changes.
    let before = p.features.len();
    let mut d = DeleteFaceOp::new();
    d.select = DeleteSelect::LumpOrVoid;
    d.faces.push(body_face(
        &p,
        A,
        |q| (q.z - H).abs() < 1e-6,
        [25.0, 15.0, H],
    ));
    refused(
        p.add_body_op(BodyOpFeature::new(BodyOp::DeleteFace(d))),
        "one lump and no void",
    );
    assert_eq!(p.features.len(), before);
    assert_eq!(names(&p), vec![A.to_owned(), B.to_owned()]);
    near(vol(&p, A), HALF, "A kept");
}

#[test]
fn editing_the_split_moves_both_bodies_features() {
    let mut p = split_plate();
    shell_top(&mut p, A, [25.0, 15.0, H]);
    shell_top(&mut p, B, [75.0, 15.0, H]);
    // The work plane (feature 1) from x = 50 to x = 40.
    let Feature::Work(mut w) = p.features[1].clone() else {
        panic!("{:?}", p.features[1])
    };
    w.def = WorkDef::Plane(WorkPlaneDef::Offset {
        base: PlaneRef::Origin(OriginPlane::YZ),
        distance: 40.0,
    });
    p.replace_feature(1, Feature::Work(w)).unwrap();
    healthy(&p);
    near(vol(&p, A), shelled(40.0, D, H), "A after the edit");
    near(vol(&p, B), shelled(60.0, D, H), "B after the edit");
}

#[test]
fn suppressing_the_split_suppresses_what_uses_its_body() {
    let mut p = split_plate();
    shell_top(&mut p, A, [25.0, 15.0, H]);
    shell_top(&mut p, B, [75.0, 15.0, H]);
    let k = p.features.iter().position(|f| f.id() == "Split1").unwrap();
    let off = p.suppress_feature(k).unwrap();
    // B's shell needs B: it goes with the Split. A's shell stays, now on
    // the whole plate (its top face is the plate's top).
    assert!(off.iter().any(|s| s == "Shell2"), "{off:?}");
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    assert_eq!(names(&p), vec![A.to_owned()]);
    near(vol(&p, A), shelled(W, D, H), "the whole plate shelled");
    p.unsuppress_feature(k).unwrap();
    healthy(&p);
    near(vol(&p, A), shelled(50.0, D, H), "A");
    near(vol(&p, B), shelled(50.0, D, H), "B");
}

#[test]
fn a_multi_body_part_saves_and_loads() {
    let mut p = split_plate();
    shell_top(&mut p, A, [25.0, 15.0, H]);
    boss_on_end(&mut p, 5.0, &[B]);
    add_move(&mut p, B, 0.0, 50.0, 0.0);
    healthy(&p);
    let q = save_load(&p, "chain");
    healthy(&q);
    assert_eq!(q.features, p.features);
    assert_eq!(names(&q), names(&p));
    assert_eq!(q.bodies_before(usize::MAX), p.bodies_before(usize::MAX));
    for n in [A, B] {
        near(vol(&q, n), vol(&p, n), n);
        assert_eq!(bbox(&q, n), bbox(&p, n), "{n} box");
    }
}

/// The owner's box: 200 × 80 × 20, split at z = 13 into a base (below) and
/// a lid, both shelled 2 mm from the cut; a knob on the lid joins only the
/// lid, a hole through the base's floor cuts only the base, then the lid is
/// lifted off (exploded) and the file saved and loaded.
#[test]
fn the_owners_box_base_and_lid() {
    const BW: Real = 200.0;
    const BD: Real = 80.0;
    const CUT: Real = 13.0;
    let mut p = plate(BW, BD, H);
    let wp = p
        .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
            base: PlaneRef::Origin(OriginPlane::XY),
            distance: CUT,
        })))
        .unwrap();
    let mut s = SplitFeature::new(SplitMethod::SplitSolid);
    s.tool = Some(SplitToolRef::Plane(PlaneRef::Work(wp)));
    p.add_split(s).unwrap();
    // Which body is the lid (above the cut)?
    let lid = names(&p)
        .into_iter()
        .find(|n| bbox(&p, n).0.z > CUT - 1e-6)
        .expect("a lid");
    let base = names(&p).into_iter().find(|n| *n != lid).unwrap();
    for (cut, at) in [
        ("Split1:Cut", [BW / 2.0, 1.0, CUT]),
        ("Split1#2:Cut", [BW / 2.0, 1.0, CUT]),
    ] {
        let mut sh = ShellFeature::new(2.0);
        sh.remove.push(face_ref(&p, cut, at));
        p.add_shell(sh)
            .unwrap_or_else(|e| panic!("shell {cut}: {e}"));
    }
    healthy(&p);
    let tray = |h: Real| BW * BD * h - (BW - 4.0) * (BD - 4.0) * (h - 2.0);
    near(vol(&p, &base), tray(CUT), "base");
    near(vol(&p, &lid), tray(H - CUT), "lid");
    // A knob Ø20 × 5 on the lid's top, joined to the lid only.
    let lid_top = top(&p, H);
    let j = sketch_on(&mut p, &lid_top, |sk, at| {
        sk.add_circle(at(BW / 2.0, BD / 2.0, H), 10.0).unwrap();
    });
    let r = smallest(&p.sketches[j].sketch.regions());
    let mut ex = ExtrudeFeature::new(r, 5.0);
    ex.operation = Operation::Join;
    ex.solids = vec![lid.clone()];
    p.add_extrude(j, ex).unwrap();
    healthy(&p);
    let knob = PI * 100.0 * 5.0;
    near(vol(&p, &lid), tray(H - CUT) + knob, "lid with the knob");
    // A hole Ø8 through the base only: its floor (2 mm) loses a disc, the
    // lid above it (top 2 mm, and the knob) stays whole.
    holes(&mut p, &[(30.0, 30.0)], 4.0, &[&base]);
    healthy(&p);
    near(vol(&p, &base), tray(CUT) - PI * 16.0 * 2.0, "base drilled");
    near(vol(&p, &lid), tray(H - CUT) + knob, "lid not drilled");
    // Lift the lid 30 mm.
    add_move(&mut p, &lid, 0.0, 0.0, 30.0);
    healthy(&p);
    // (Display points: the hole's wall mesh may sag a few µm.)
    assert!((bbox(&p, &lid).0.z - (CUT + 30.0)).abs() < 1e-3);
    assert!(bbox(&p, &base).0.z.abs() < 1e-3, "{:?}", bbox(&p, &base));
    let q = save_load(&p, "box");
    healthy(&q);
    for n in [&base, &lid] {
        near(vol(&q, n), vol(&p, n), n);
    }
}

#[test]
fn face_draft_of_a_shared_face_name_takes_the_hinted_body() {
    // The plate's front (y = 0) is cut in two by the Split: both pieces keep
    // its name. Drafting B's piece must not draft A's.
    let mut p = split_plate();
    let front = body_face(&p, B, |q| q.y.abs() < 1e-6, [75.0, 0.0, 10.0]);
    let a_front = body_face(&p, A, |q| q.y.abs() < 1e-6, [25.0, 0.0, 10.0]);
    assert_eq!(front.face, a_front.face, "one name on both bodies");
    let mut d = DraftFeature::new(5f64.to_radians());
    d.draft_type = DraftType::FixedPlane;
    d.plane = Some(PlaneRef::Origin(OriginPlane::XY));
    d.faces.push(front);
    p.add_draft(d).unwrap();
    healthy(&p);
    near(vol(&p, A), HALF, "A untouched");
    // A 50 × 20 wall tilted 5° about its foot: a wedge 50 · 20² · tan 5° / 2.
    let wedge = 50.0 * H * H * k(5.0) / 2.0;
    let dv = (vol(&p, B) - HALF).abs();
    assert!((dv - wedge).abs() < 1e-4 * wedge, "{dv} vs {wedge}");
}
