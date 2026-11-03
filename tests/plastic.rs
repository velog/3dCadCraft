//! Plastic part features (spec 5.8): volumes worked out by hand from each option's meaning.

use std::f64::consts::PI;

use cadcraft3d::kernel::{Point3, Real};
use cadcraft3d::part::blend::edge_ref;
use cadcraft3d::part::plastic::boss::{BossHead, BossKind, BossOp, BossRibs, BossThread, ThreadHole};
use cadcraft3d::part::plastic::grill::{GrillCurves, GrillOp, flow_area, regions_of};
use cadcraft3d::part::plastic::lip::{LipKind, LipOp};
use cadcraft3d::part::plastic::rest::{RestExtent, RestOp, RestSide};
use cadcraft3d::part::plastic::rule_fillet::{FilletRule, RuleFilletOp, RuleRow, RuleSource};
use cadcraft3d::part::plastic::snap_fit::{SnapFitOp, SnapHook, SnapKind, SnapLoop};
use cadcraft3d::part::work::{LineRef, WorkPlaneDef};
use cadcraft3d::part::{
    BodyOp, BodyOpFeature, EdgeRef, ExtrudeFeature, FaceRef, OriginPlane, PartDocument, PlaneRef,
    ProfileRef, SketchOptions, SketchPlacement, WorkDef, WorkFeature,
};
use nalgebra::Point2;

const PLAIN: fn() -> SketchOptions = || SketchOptions {
    auto_project_origin: false,
    auto_reference_edges: false,
    ..Default::default()
};

fn p2(x: Real, y: Real) -> Point2<Real> {
    Point2::new(x, y)
}

fn volume(p: &PartDocument) -> Real {
    p.exact_bodies
        .0
        .iter()
        .filter(|s| !s.is_open())
        .filter_map(|s| s.volume_at(1e-3))
        .sum()
}

fn close(v: Real, want: Real, tol: Real, what: &str) {
    assert!((v - want).abs() < tol, "{what}: {v}, expected {want}");
}

fn plane_at(p: &mut PartDocument, z: Real) -> usize {
    let wp = p
        .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
            base: PlaneRef::Origin(OriginPlane::XY),
            distance: z,
        })))
        .unwrap_or_else(|e| panic!("{e}"));
    p.create_sketch(
        SketchPlacement {
            plane: PlaneRef::Work(wp),
            ..Default::default()
        },
        &PLAIN(),
    )
    .unwrap_or_else(|e| panic!("{e}"))
}

/// A wall 100 x 60 x 3, z from 0 to 3.
fn wall() -> PartDocument {
    PartDocument::with_box("Box1", [100.0, 60.0, 3.0]).unwrap_or_else(|e| panic!("{e}"))
}

fn add(p: &mut PartDocument, op: BodyOp) -> String {
    p.add_body_op(BodyOpFeature::new(op))
        .unwrap_or_else(|e| panic!("{e}"))
}

/// A sketch at `z` with a Center Point at (50, 30).
fn center_at(p: &mut PartDocument, z: Real) -> String {
    let i = plane_at(p, z);
    p.sketches[i].sketch.add_center_point(p2(50.0, 30.0));
    p.sketches[i].id.clone()
}

const V0: Real = 18000.0;

#[test]
fn thread_boss_holes() {
    let post = PI * 9.0 * 17.0;
    let hole = PI * 1.5625;
    for (kind, want) in [
        (ThreadHole::None, post),
        (ThreadHole::FullDepth, post - hole * 17.0),
        (ThreadHole::Depth(5.0), post - hole * 5.0),
        (ThreadHole::Through, post - hole * 20.0),
    ] {
        let mut p = wall();
        let id = center_at(&mut p, 20.0);
        let mut op = BossOp::new(id);
        op.kind = BossKind::Thread(BossThread {
            hole: kind,
            ..BossThread::default()
        });
        add(&mut p, BodyOp::Boss(op));
        close(volume(&p), V0 + want, 0.2, &format!("{kind:?}"));
    }
}

#[test]
fn head_boss_counterbore() {
    let mut p = wall();
    let id = center_at(&mut p, 20.0);
    let mut op = BossOp::new(id);
    op.kind = BossKind::Head(BossHead::default());
    add(&mut p, BodyOp::Boss(op));
    // The post Ø11 in the air; shank hole, the groove round the seat ring, the recess Ø8.
    let post = PI * 30.25 * 17.0;
    let cut = PI * (2.25 * 1.5 + (2.25 + 16.0 - 12.25) * 0.5 + 16.0 * 18.0);
    close(volume(&p), V0 + post - cut, 0.3, "counterbore");
}

#[test]
fn boss_offset_and_flip_and_errors() {
    // Offset 2: the boss starts 2 above the sketch plane, so it is 2 longer.
    let mut p = wall();
    let id = center_at(&mut p, 20.0);
    let mut op = BossOp::new(id);
    op.offset = 2.0;
    op.kind = BossKind::Thread(BossThread {
        hole: ThreadHole::None,
        ..BossThread::default()
    });
    add(&mut p, BodyOp::Boss(op));
    close(volume(&p), V0 + PI * 9.0 * 19.0, 0.2, "offset");
    // Flipped away from the only wall: refused, the part is as it was.
    let mut p = wall();
    let id = center_at(&mut p, 20.0);
    let mut op = BossOp::new(id);
    op.flip = true;
    let n = p.features.len();
    assert!(p.add_body_op(BodyOpFeature::new(BodyOp::Boss(op))).is_err());
    assert_eq!(p.features.len(), n);
    close(volume(&p), V0, 1e-6, "unchanged");
}

/// Area of the strip |y| ≤ t/2, x from the circle of radius r out to `out`.
fn strip_outside(r: Real, t: Real, out: Real) -> Real {
    let h = t / 2.0;
    // ∫ sqrt(r² − y²) dy over ±h.
    let inner = h * (r * r - h * h).sqrt() + r * r * (h / r).asin();
    2.0 * h * out - inner
}

#[test]
fn boss_ribs() {
    let mut p = wall();
    let id = center_at(&mut p, 20.0);
    let mut op = BossOp::new(id);
    op.kind = BossKind::Thread(BossThread {
        hole: ThreadHole::None,
        ..BossThread::default()
    });
    op.ribs = Some(BossRibs {
        count: 4,
        thickness: 1.0,
        shoulder: 3.0,
        top_offset: 2.0,
        ..BossRibs::default()
    });
    add(&mut p, BodyOp::Boss(op));
    let rib = strip_outside(3.0, 1.0, 6.0) * 15.0;
    close(volume(&p), V0 + PI * 9.0 * 17.0 + 4.0 * rib, 0.2, "ribs");
}

#[test]
fn boss_fillet_at_the_body() {
    let mut p = wall();
    let id = center_at(&mut p, 20.0);
    let mut op = BossOp::new(id);
    op.kind = BossKind::Thread(BossThread {
        hole: ThreadHole::None,
        ..BossThread::default()
    });
    op.fillet = 1.0;
    add(&mut p, BodyOp::Boss(op));
    close(
        volume(&p),
        V0 + PI * 9.0 * 17.0 + concave_ring(3.0, 1.0),
        0.2,
        "fillet",
    );
}

/// The material of a concave fillet ρ round a post of radius r (Pappus).
fn concave_ring(r: Real, rho: Real) -> Real {
    let area = (1.0 - PI / 4.0) * rho * rho;
    let x = (10.0 - 3.0 * PI) / (3.0 * (4.0 - PI)) * rho;
    2.0 * PI * (r + x) * area
}

/// A hook's material above the wall: the tapered beam and the head.
#[test]
fn snap_hook() {
    let h = SnapHook::default();
    let mut p = wall();
    // The point stands 2 above the wall, so the root goes on 2 + half the wall.
    let id = center_at(&mut p, 5.0);
    let mut op = SnapFitOp::new(id);
    op.kind = SnapKind::Hook(h.clone());
    add(&mut p, BodyOp::SnapFit(op));
    let back = 2.0 + 1.5;
    let total = h.length + back;
    let t = |z: Real| h.thickness_wall + (h.thickness_hook - h.thickness_wall) * (z + back) / total;
    let beam = (t(-2.0) + t(h.length)) / 2.0 * (h.length + 2.0) * h.width_wall;
    let (z_r, z_tip) = (h.length - h.hook_length, h.length - h.tip_length);
    let z_ramp = z_tip - h.undercut / h.insertion.tan();
    let x = |z: Real| t(z) / 2.0;
    let poly = [
        (x(z_r), z_r),
        (x(z_r) + h.undercut, z_r),
        (x(z_r) + h.undercut, z_ramp),
        (x(z_tip), z_tip),
    ];
    let area: Real = (0..4)
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % 4]);
            a.0 * b.1 - b.0 * a.1
        })
        .sum::<Real>()
        / 2.0;
    close(volume(&p), V0 + beam + area * h.width_hook, 0.02, "hook");
}

#[test]
fn snap_loop_and_stop_at_point() {
    let l = SnapLoop {
        thickness_top: 1.5,
        ..SnapLoop::default()
    };
    // Stop at the point: the point lies on the wall's top, the loop stands on it from there.
    let mut p = wall();
    let id = center_at(&mut p, 3.0);
    let mut op = SnapFitOp::new(id);
    op.kind = SnapKind::Loop(l.clone());
    op.extend = false;
    add(&mut p, BodyOp::SnapFit(op));
    let frame = l.width * l.length * l.thickness_wall;
    let opening = (l.width - 2.0 * l.catch_side) * l.opening * l.thickness_wall;
    close(volume(&p), V0 + frame - opening, 0.02, "loop");
    // A hook faces each of the four arrows: same volume, different box.
    let mut seen = Vec::new();
    for arrow in 0..4u8 {
        let mut p = wall();
        let id = center_at(&mut p, 5.0);
        let mut op = SnapFitOp::new(id);
        op.hook_arrow = arrow;
        add(&mut p, BodyOp::SnapFit(op));
        // Where the hook's land stands: the farthest material high above the wall.
        let mut far = (0.0, 0.0);
        for e in p.body.visible_edges() {
            for q in p.body.sample_edge(e, &Default::default()).unwrap_or_default() {
                if q.z > 8.0 && (q.x - 50.0).abs() + (q.y - 30.0).abs() > far.0 {
                    far = ((q.x - 50.0).abs() + (q.y - 30.0).abs(), (q.x - 50.0).atan2(q.y - 30.0));
                }
            }
        }
        seen.push((far, volume(&p)));
    }
    for s in &seen {
        close(s.1, seen[0].1, 1e-3, "same volume");
        assert!(s.0.0 > 1.0, "the hook stands out: {:?}", s.0);
    }
    assert!((seen[0].0.1 - seen[2].0.1).abs() > 1.0, "the hook turned: {seen:?}");
}

/// A plate with a post made by an Extrude: its id.
fn post() -> (PartDocument, String) {
    let mut p = wall();
    let i = plane_at(&mut p, 3.0);
    let sk = &mut p.sketches[i].sketch;
    sk.add_circle(p2(50.0, 30.0), 3.0).unwrap_or_else(|e| panic!("{e}"));
    let r: Vec<ProfileRef> = sk
        .regions()
        .iter()
        .map(|r| ProfileRef::of(r, r.interior_point()))
        .collect();
    let mut ex = ExtrudeFeature::new(r, 10.0);
    ex.operation = cadcraft3d::part::Operation::Join;
    let id = p.add_extrude(i, ex).unwrap_or_else(|e| panic!("{e}"));
    (p, id)
}

#[test]
fn rule_fillet_against_part_and_free_edges() {
    let base = V0 + PI * 9.0 * 10.0;
    // Against Part: only the circle at the foot of the post.
    let (mut p, id) = post();
    close(volume(&p), base, 0.1, "post");
    let row = RuleRow::features(vec![id], FilletRule::AgainstPart, 1.0);
    add(
        &mut p,
        BodyOp::RuleFillet(RuleFilletOp {
            rows: vec![row],
            exclude_faces: Vec::new(),
            remove_merged: false,
        }),
    );
    close(volume(&p), base + concave_ring(3.0, 1.0), 0.1, "against part");
    // Free Edges: the rim on top, a convex round.
    let (mut p, id) = post();
    let row = RuleRow::features(vec![id], FilletRule::FreeEdges, 1.0);
    add(
        &mut p,
        BodyOp::RuleFillet(RuleFilletOp {
            rows: vec![row],
            exclude_faces: Vec::new(),
            remove_merged: false,
        }),
    );
    let area = (1.0 - PI / 4.0) * 1.0;
    let x = 1.0 - (10.0 - 3.0 * PI) / (3.0 * (4.0 - PI));
    let gone = 2.0 * PI * (2.0 + x) * area;
    close(volume(&p), base - gone, 0.1, "free edges");
    // All Rounds off: Against Part stays (a concave edge); All Fillets off: nothing is left to
    // round, so the part stays as it was (a rule that matches nothing is skipped).
    let (mut p, id) = post();
    let mut row = RuleRow::features(vec![id], FilletRule::AgainstPart, 1.0);
    row.all_fillets = false;
    add(
        &mut p,
        BodyOp::RuleFillet(RuleFilletOp {
            rows: vec![row],
            exclude_faces: Vec::new(),
            remove_merged: false,
        }),
    );
    close(volume(&p), base, 0.1, "nothing convex against the part");
}

#[test]
fn rule_fillet_two_rows_follow_the_model() {
    let base = V0 + PI * 9.0 * 10.0;
    let (mut p, id) = post();
    let rows = vec![
        RuleRow::features(vec![id.clone()], FilletRule::FreeEdges, 0.5),
        RuleRow::features(vec![id], FilletRule::AgainstPart, 1.0),
    ];
    add(
        &mut p,
        BodyOp::RuleFillet(RuleFilletOp {
            rows,
            exclude_faces: Vec::new(),
            remove_merged: false,
        }),
    );
    let (a, x) = (
        (1.0 - PI / 4.0) * 0.25,
        0.5 * (1.0 - (10.0 - 3.0 * PI) / (3.0 * (4.0 - PI))),
    );
    let rim = 2.0 * PI * (2.5 + x) * a;
    close(
        volume(&p),
        base + concave_ring(3.0, 1.0) - rim,
        0.1,
        "both rows",
    );
}

#[test]
fn rule_fillet_incident_edges() {
    // The four upright edges of a box, from its bottom face up.
    let mut p = PartDocument::with_box("Box1", [20.0, 20.0, 10.0]).unwrap_or_else(|e| panic!("{e}"));
    let row = RuleRow {
        source: RuleSource::Faces(vec![FaceRef {
            face: "Box1:Bottom".into(),
            hint: [10.0, 10.0, 0.0],
        }]),
        rule: FilletRule::IncidentEdges {
            axis: LineRef::Origin(2),
            flip: false,
            tolerance: 1f64.to_radians(),
        },
        radius: 2.0,
        all_rounds: true,
        all_fillets: true,
    };
    add(
        &mut p,
        BodyOp::RuleFillet(RuleFilletOp {
            rows: vec![row.clone()],
            exclude_faces: Vec::new(),
            remove_merged: false,
        }),
    );
    let gone = 4.0 * (1.0 - PI / 4.0) * 4.0 * 10.0;
    close(volume(&p), 4000.0 - gone, 0.1, "incident");
    // The axis the other way: no edge rises against it, the part stays.
    let mut p = PartDocument::with_box("Box1", [20.0, 20.0, 10.0]).unwrap_or_else(|e| panic!("{e}"));
    let mut flipped = row;
    if let FilletRule::IncidentEdges { flip, .. } = &mut flipped.rule {
        *flip = true;
    }
    add(
        &mut p,
        BodyOp::RuleFillet(RuleFilletOp {
            rows: vec![flipped],
            exclude_faces: Vec::new(),
            remove_merged: false,
        }),
    );
    close(volume(&p), 4000.0, 1e-6, "flipped");
}

#[test]
fn rest_on_a_flat_wall() {
    let mut p = wall();
    let i = plane_at(&mut p, 8.0);
    p.sketches[i]
        .sketch
        .add_rectangle_two_point(p2(40.0, 20.0), p2(60.0, 40.0))
        .unwrap_or_else(|e| panic!("{e}"));
    let id = p.sketches[i].id.clone();
    add(&mut p, BodyOp::Rest(RestOp::new(id, 1.0)));
    // The hole inside the wall through the plate; walls 5 up to the landing; the landing itself.
    let want = V0 - 324.0 * 3.0 + 76.0 * 5.0 + 324.0;
    close(volume(&p), want, 0.02, "rest");
}

#[test]
fn rest_sides_and_distance() {
    for (side, outer, inner) in [
        (RestSide::Outside, 22.0, 20.0),
        (RestSide::BothSides, 22.0, 18.0),
    ] {
        let mut p = wall();
        let i = plane_at(&mut p, 8.0);
        p.sketches[i]
            .sketch
            .add_rectangle_two_point(p2(40.0, 20.0), p2(60.0, 40.0))
            .unwrap_or_else(|e| panic!("{e}"));
        let id = p.sketches[i].id.clone();
        let mut op = RestOp::new(id, if side == RestSide::Outside { 1.0 } else { 2.0 });
        op.side = side;
        let t = if side == RestSide::Outside { 1.0 } else { 2.0 };
        add(&mut p, BodyOp::Rest(op));
        let (ao, ai) = (outer * outer, inner * inner);
        // The hole through the plate, walls up to the landing, the landing t thick.
        let want = V0 - ai * 3.0 + (ao - ai) * 5.0 + ai * t;
        close(volume(&p), want, 0.02, &format!("{side:?}"));
    }
    // Distance 2: walls stop in the air, 3 above the plate; the wall below stays open.
    let mut p = wall();
    let i = plane_at(&mut p, 8.0);
    p.sketches[i]
        .sketch
        .add_rectangle_two_point(p2(40.0, 20.0), p2(60.0, 40.0))
        .unwrap_or_else(|e| panic!("{e}"));
    let id = p.sketches[i].id.clone();
    let mut op = RestOp::new(id, 1.0);
    op.extent = RestExtent::Distance(2.0);
    add(&mut p, BodyOp::Rest(op));
    let want = V0 - 324.0 * 3.0 + 76.0 * 2.0 + 324.0;
    close(volume(&p), want, 0.02, "distance");
}

/// A wall sloping in X: z from 10 + 0.1x to 13 + 0.1x, y from 0 to 60.
fn slanted() -> PartDocument {
    let mut p = PartDocument::default();
    let i = p
        .create_sketch(
            SketchPlacement {
                plane: PlaneRef::Origin(OriginPlane::XZ),
                ..Default::default()
            },
            &PLAIN(),
        )
        .unwrap_or_else(|e| panic!("{e}"));
    let sk = &mut p.sketches[i].sketch;
    let pts = [(0.0, 10.0), (100.0, 20.0), (100.0, 23.0), (0.0, 13.0)];
    for k in 0..4 {
        let (a, b) = (pts[k], pts[(k + 1) % 4]);
        sk.add_line(p2(a.0, a.1), p2(b.0, b.1));
    }
    // Lines meet by position: regions find them.
    let r: Vec<ProfileRef> = sk
        .regions()
        .iter()
        .map(|r| ProfileRef::of(r, r.interior_point()))
        .collect();
    let mut ex = ExtrudeFeature::new(r, 60.0);
    ex.operation = cadcraft3d::part::Operation::NewSolid;
    p.add_extrude(i, ex).unwrap_or_else(|e| panic!("{e}"));
    p
}

#[test]
fn rest_on_a_slanted_wall() {
    let mut p = slanted();
    let v0 = volume(&p);
    close(v0, 18000.0, 0.01, "slanted wall");
    // Sign of the part's extrusion is the sketch's: the wall may run either way along Y.
    let (lo, hi) = p.body.bounding_box().unwrap_or_else(|| panic!("no box"));
    let mid_y = (lo.y + hi.y) / 2.0;
    let i = plane_at(&mut p, 16.0);
    p.sketches[i]
        .sketch
        .add_rectangle_two_point(p2(40.0, mid_y - 10.0), p2(60.0, mid_y + 10.0))
        .unwrap_or_else(|e| panic!("{e}"));
    let id = p.sketches[i].id.clone();
    let mut op = RestOp::new(id, 1.0);
    // The plane cuts the wall: downwards is the platform's way here.
    op.flip = true;
    add(&mut p, BodyOp::Rest(op));
    // What should stand, summed on a fine grid over the part's box.
    let step = 0.1;
    let in_po = |x: Real, y: Real| (40.0..60.0).contains(&x) && (y - mid_y).abs() < 10.0;
    let in_pi = |x: Real, y: Real| (41.0..59.0).contains(&x) && (y - mid_y).abs() < 9.0;
    let mut change = 0.0;
    let mut x = 39.05;
    while x < 61.0 {
        let mut y = mid_y - 11.0 + step / 2.0;
        while y < mid_y + 11.0 {
            if in_po(x, y) {
                let (w0, w1, mid) = (10.0 + 0.1 * x, 13.0 + 0.1 * x, 11.5 + 0.1 * x);
                let mut z = 9.05;
                while z < 27.0 {
                    let wall = (w0..w1).contains(&z);
                    let plate = (15.0..16.0).contains(&z);
                    let ring = (z < 16.0 && z > mid) || (z > 16.0 && z < mid);
                    let now = if in_pi(x, y) {
                        plate
                    } else {
                        wall || plate || ring
                    };
                    change += (now as i32 - wall as i32) as Real;
                    z += step;
                }
            }
            y += step;
        }
        x += step;
    }
    let want = v0 + change * step * step * step;
    close(volume(&p), want, want * 2e-3, "slanted rest");
}

fn grill_sketch(p: &mut PartDocument) -> usize {
    let i = plane_at(p, 10.0);
    p.sketches[i]
        .sketch
        .add_rectangle_two_point(p2(30.0, 20.0), p2(70.0, 40.0))
        .unwrap_or_else(|e| panic!("{e}"));
    i
}

fn lines(p: &mut PartDocument, i: usize, l: &[((Real, Real), (Real, Real))]) -> Vec<cadcraft3d::sketch::EntityId> {
    l.iter()
        .map(|(a, b)| p.sketches[i].sketch.add_line(p2(a.0, a.1), p2(b.0, b.1)))
        .collect()
}

#[test]
fn grill_boundary_ribs_spars() {
    // Boundary only.
    let mut p = wall();
    let i = grill_sketch(&mut p);
    let id = p.sketches[i].id.clone();
    let mut op = GrillOp::new(id.clone());
    op.outside_height = 1.0;
    let boundary = op.clone();
    add(&mut p, BodyOp::Grill(op));
    close(volume(&p), V0 - 684.0 * 3.0 + 116.0, 0.02, "boundary");
    close(
        flow_area(&boundary, &p.sketches).unwrap_or_else(|e| panic!("{e}")),
        684.0,
        0.05,
        "flow",
    );

    // Ribs, then spars across them.
    let mut p = wall();
    let i = grill_sketch(&mut p);
    let id = p.sketches[i].id.clone();
    let ribs = lines(
        &mut p,
        i,
        &[
            ((40.0, 15.0), (40.0, 45.0)),
            ((50.0, 15.0), (50.0, 45.0)),
            ((60.0, 15.0), (60.0, 45.0)),
        ],
    );
    let spars = lines(&mut p, i, &[((25.0, 30.0), (75.0, 30.0))]);
    let mut op = GrillOp::new(id.clone());
    op.outside_height = 1.0;
    op.ribs = Some(GrillCurves {
        sketch: id.clone(),
        curves: ribs,
        thickness: 1.0,
        height: 2.0,
        top_offset: 0.5,
        bottom_offset: 0.0,
    });
    op.spars = Some(GrillCurves {
        sketch: id,
        curves: spars,
        thickness: 1.0,
        height: 0.0,
        top_offset: 0.5,
        bottom_offset: 0.5,
    });
    let def = op.clone();
    add(&mut p, BodyOp::Grill(op));
    // Three ribs 18 x 1 x 2; the spar 38 x 1 x 2 less its crossings with them.
    let want = V0 - 684.0 * 3.0 + 116.0 + 108.0 + 76.0 - 4.5;
    close(volume(&p), want, 0.02, "ribs and spar");
    close(
        flow_area(&def, &p.sketches).unwrap_or_else(|e| panic!("{e}")),
        684.0 - 54.0 - 35.0,
        0.05,
        "flow with ribs and spar",
    );
}

#[test]
fn grill_island_stops_ribs() {
    let mut p = wall();
    let i = grill_sketch(&mut p);
    let id = p.sketches[i].id.clone();
    let ribs = lines(&mut p, i, &[((50.0, 15.0), (50.0, 45.0))]);
    // The island in the boundary's own sketch: the ribs' lines must not cut either of them.
    p.sketches[i]
        .sketch
        .add_rectangle_two_point(p2(45.0, 25.0), p2(55.0, 35.0))
        .unwrap_or_else(|e| panic!("{e}"));
    let mut def0 = GrillOp::new(id.clone());
    def0.ribs = Some(GrillCurves {
        sketch: id.clone(),
        curves: ribs.clone(),
        thickness: 1.0,
        height: 2.0,
        top_offset: 0.0,
        bottom_offset: 0.0,
    });
    let regions = regions_of(&def0, &p.sketches[i].sketch, &id);
    assert_eq!(regions.len(), 2, "boundary and island");
    let of = |small: bool| -> Vec<ProfileRef> {
        regions
            .iter()
            .filter(|r| (r.outer.area().abs() < 300.0) == small)
            .map(|r| ProfileRef::of(r, r.interior_point()))
            .collect()
    };
    let (islands, boundary) = (of(true), of(false));
    let mut op = GrillOp::new(id.clone());
    op.boundary = boundary.first().cloned();
    op.outside_height = 1.0;
    op.islands = islands;
    op.ribs = Some(GrillCurves {
        sketch: id,
        curves: ribs,
        thickness: 1.0,
        height: 2.0,
        top_offset: 0.0,
        bottom_offset: 0.0,
    });
    let def = op.clone();
    add(&mut p, BodyOp::Grill(op));
    // The island 10 x 10 x 2 from level 4 to 2; the rib 8 long, at the boundary's own levels.
    let want = V0 - 684.0 * 3.0 + 116.0 + 200.0 + 16.0;
    close(volume(&p), want, 0.02, "island");
    close(
        flow_area(&def, &p.sketches).unwrap_or_else(|e| panic!("{e}")),
        684.0 - 100.0 - 8.0,
        0.05,
        "flow",
    );
}

#[test]
fn grill_refuses_an_outline_off_the_part() {
    let mut p = wall();
    let i = plane_at(&mut p, 10.0);
    p.sketches[i]
        .sketch
        .add_rectangle_two_point(p2(90.0, 20.0), p2(120.0, 40.0))
        .unwrap_or_else(|e| panic!("{e}"));
    let id = p.sketches[i].id.clone();
    let n = p.features.len();
    assert!(
        p.add_body_op(BodyOpFeature::new(BodyOp::Grill(GrillOp::new(id))))
            .is_err()
    );
    assert_eq!(p.features.len(), n);
}

/// A rectangular ring: outside 100 x 60, walls 3, z from 0 to 10. Its inner top edges.
fn ring() -> (PartDocument, Vec<EdgeRef>, Vec<EdgeRef>) {
    let mut p = PartDocument::default();
    let i = p
        .create_sketch(
            SketchPlacement {
                plane: PlaneRef::Origin(OriginPlane::XY),
                ..Default::default()
            },
            &PLAIN(),
        )
        .unwrap_or_else(|e| panic!("{e}"));
    let sk = &mut p.sketches[i].sketch;
    sk.add_rectangle_two_point(p2(0.0, 0.0), p2(100.0, 60.0))
        .unwrap_or_else(|e| panic!("{e}"));
    sk.add_rectangle_two_point(p2(3.0, 3.0), p2(97.0, 57.0))
        .unwrap_or_else(|e| panic!("{e}"));
    let r: Vec<ProfileRef> = sk
        .regions()
        .iter()
        .filter(|r| !r.holes.is_empty())
        .map(|r| ProfileRef::of(r, r.interior_point()))
        .collect();
    let mut ex = ExtrudeFeature::new(r, 10.0);
    ex.operation = cadcraft3d::part::Operation::NewSolid;
    p.add_extrude(i, ex).unwrap_or_else(|e| panic!("{e}"));
    let (inner, outer) = rim_edges(&p);
    (p, inner, outer)
}

/// The rim's inner and outer edges on the top plane.
fn rim_edges(p: &PartDocument) -> (Vec<EdgeRef>, Vec<EdgeRef>) {
    let (zmax, body) = (
        p.body
            .bounding_box()
            .unwrap_or_else(|| panic!("no box"))
            .1
            .z,
        &p.body,
    );
    let (mut inner, mut outer) = (Vec::new(), Vec::new());
    for e in body.visible_edges() {
        let Ok(pts) = body.sample_edge(e, &Default::default()) else {
            continue;
        };
        if !pts.iter().all(|q| (q.z - zmax).abs() < 1e-6) {
            continue;
        }
        let Some(r) = edge_ref(body, e) else {
            continue;
        };
        let m = Point3::from(r.hint);
        let out = m.x < 1e-6 || m.y < 1e-6 || m.x > 100.0 - 1e-6 || m.y > 60.0 - 1e-6;
        if out { outer.push(r) } else { inner.push(r) }
    }
    (inner, outer)
}

const RING_V: Real = (6000.0 - 94.0 * 54.0) * 10.0;

#[test]
fn lip_and_groove_on_a_closed_rim() {
    let inside = 94.0 * 54.0;
    let bead = 96.0 * 56.0 - inside;
    // Lip: 1 x 1 beside the inner edge.
    let (mut p, inner, _) = ring();
    close(volume(&p), RING_V, 1e-3, "ring");
    assert_eq!(inner.len(), 4);
    let mut op = LipOp::new(inner.clone(), 1.0, 1.0);
    op.pull = Some(LineRef::Origin(2));
    add(&mut p, BodyOp::Lip(op));
    close(volume(&p), RING_V + bead, 0.02, "lip");
    // Groove: the same bead cut out.
    let (mut p, inner, _) = ring();
    let mut op = LipOp::new(inner, 1.0, 1.0);
    op.kind = LipKind::Groove;
    op.pull = Some(LineRef::Origin(2));
    add(&mut p, BodyOp::Lip(op));
    close(volume(&p), RING_V - bead, 0.02, "groove");
}

#[test]
fn lip_clearance_shoulder_and_draft() {
    let inside = 94.0 * 54.0;
    let area = |d: Real| (94.0 + 2.0 * d) * (54.0 + 2.0 * d) - inside;
    // Clearance: the rim beyond the bead is 0.5 lower.
    let (mut p, inner, _) = ring();
    let mut op = LipOp::new(inner, 1.0, 1.0);
    op.pull = Some(LineRef::Origin(2));
    op.clearance = 0.5;
    add(&mut p, BodyOp::Lip(op));
    let beyond = 6000.0 - (96.0 * 56.0);
    close(volume(&p), RING_V + area(1.0) - beyond * 0.5, 0.02, "clearance");
    // Shoulder: the bead stands 1 from the path.
    let (mut p, inner, _) = ring();
    let mut op = LipOp::new(inner, 1.0, 1.0);
    op.pull = Some(LineRef::Origin(2));
    op.shoulder = 1.0;
    add(&mut p, BodyOp::Lip(op));
    close(volume(&p), RING_V + area(2.0) - area(1.0), 0.02, "shoulder");
    // Open path: one edge, the bead's ends square to it.
    let (mut p, inner, _) = ring();
    let long = inner
        .iter()
        .find(|e| (e.hint[1] - 3.0).abs() < 1e-6)
        .cloned()
        .unwrap_or_else(|| panic!("no long edge"));
    let mut op = LipOp::new(vec![long], 1.0, 1.0);
    op.pull = Some(LineRef::Origin(2));
    add(&mut p, BodyOp::Lip(op));
    close(volume(&p), RING_V + 94.0, 0.02, "open path");
    // Draft: 45° on the far wall of a bead 1 high and 2 wide; 1 wide at the top.
    let (mut p, inner, _) = ring();
    let mut op = LipOp::new(inner, 1.0, 2.0);
    op.pull = Some(LineRef::Origin(2));
    op.outside_angle = 45f64.to_radians();
    add(&mut p, BodyOp::Lip(op));
    let v = volume(&p) - RING_V;
    assert!(v > area(1.0) && v < area(2.0), "draft: {v}");
}

/// What the kernel takes today: a round on the foot or the shoulder with ribs; a refused rib round
/// names itself and leaves the part as it was.
#[test]
fn boss_rib_rounds() {
    let plain = {
        let mut p = wall();
        let id = center_at(&mut p, 20.0);
        let mut op = BossOp::new(id);
        op.ribs = Some(BossRibs {
            shoulder: 3.0,
            top_offset: 8.0,
            ..BossRibs::default()
        });
        add(&mut p, BodyOp::Boss(op));
        volume(&p)
    };
    for (name, r0, sr) in [("foot", 0.5, 0.0), ("shoulder", 0.0, 0.5)] {
        let mut p = wall();
        let id = center_at(&mut p, 20.0);
        let mut op = BossOp::new(id);
        op.fillet = r0;
        op.ribs = Some(BossRibs {
            shoulder: 3.0,
            top_offset: 8.0,
            shoulder_radius: sr,
            ..BossRibs::default()
        });
        add(&mut p, BodyOp::Boss(op));
        let v = volume(&p);
        if r0 > 0.0 {
            assert!(v > plain, "{name}: a concave round adds material: {v} vs {plain}");
        } else {
            assert!(v < plain, "{name}: a convex round takes material: {v} vs {plain}");
        }
    }
    let mut p = wall();
    let id = center_at(&mut p, 20.0);
    let mut op = BossOp::new(id);
    op.ribs = Some(BossRibs {
        shoulder: 3.0,
        top_offset: 8.0,
        blend_radius: 0.3,
        ..BossRibs::default()
    });
    let n = p.features.len();
    if let Err(e) = p.add_body_op(BodyOpFeature::new(BodyOp::Boss(op))) {
        assert!(e.to_string().contains("Blend Radius"), "{e}");
        assert_eq!(p.features.len(), n);
        close(volume(&p), V0, 1e-6, "unchanged");
    }
}

/// A box with a pocket from the top: floor 2 thick, walls 2 thick, 120 x 80 x 30.
fn tray() -> PartDocument {
    let mut p = PartDocument::with_box("Box1", [120.0, 80.0, 30.0]).unwrap_or_else(|e| panic!("{e}"));
    let j = plane_at(&mut p, 2.0);
    let sk = &mut p.sketches[j].sketch;
    sk.add_rectangle_two_point(p2(2.0, 2.0), p2(118.0, 78.0)).unwrap_or_else(|e| panic!("{e}"));
    let r: Vec<ProfileRef> = sk.regions().iter().map(|r| ProfileRef::of(r, r.interior_point())).collect();
    let mut cut = ExtrudeFeature::new(r, 40.0);
    cut.operation = cadcraft3d::part::Operation::Cut;
    p.add_extrude(j, cut).unwrap_or_else(|e| panic!("{e}"));
    p
}

/// Bosses with ribs on a tray's floor. Two in a row 20 apart along X hit a kernel mesh defect (the
/// wall's triangles overlap round the ribs' feet): refused, or built; never a damaged part.
#[test]
fn bosses_with_ribs_in_a_tray() {
    for (a, b, must) in [
        ((75.0, 20.0), (105.0, 20.0), true),
        ((85.0, 20.0), (105.0, 33.0), true),
        ((40.0, 40.0), (80.0, 40.0), true),
        ((85.0, 20.0), (105.0, 20.0), false),
    ] {
        let mut p = tray();
        let before = volume(&p);
        let i = plane_at(&mut p, 25.0);
        p.sketches[i].sketch.add_center_point(p2(a.0, a.1));
        p.sketches[i].sketch.add_center_point(p2(b.0, b.1));
        let id = p.sketches[i].id.clone();
        let mut op = BossOp::new(id);
        op.ribs = Some(BossRibs {
            shoulder: 3.0,
            top_offset: 8.0,
            ..BossRibs::default()
        });
        let n = p.features.len();
        match p.add_body_op(BodyOpFeature::new(BodyOp::Boss(op))) {
            Ok(_) => {
                let rib = strip_outside(3.0, 1.0, 6.0) * 15.0;
                let post = PI * (9.0 - 1.5625) * 23.0;
                close(volume(&p), before + 2.0 * (post + 4.0 * rib), 0.5, "two bosses");
            }
            Err(e) => {
                assert!(!must, "{a:?} {b:?}: {e}");
                assert_eq!(p.features.len(), n);
                close(volume(&p), before, 1e-6, "unchanged");
            }
        }
    }
}
