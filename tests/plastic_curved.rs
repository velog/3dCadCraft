//! Boss, Rest, Snap Fit, Grill and Rule Fillet on housings that are not rectangular (spec 5.8).

use cadcraft3d::kernel::Real;
use cadcraft3d::part::blend::edge_ref;
use cadcraft3d::part::plastic::boss::{BossOp, BossRibs};
use cadcraft3d::part::plastic::grill::GrillOp;
use cadcraft3d::part::plastic::rest::RestOp;
use cadcraft3d::part::plastic::rule_fillet::{FilletRule, RuleFilletOp, RuleRow};
use cadcraft3d::part::plastic::snap_fit::{SnapFitOp, SnapHook, SnapKind};
use cadcraft3d::part::shell::{FaceRef, ShellFeature};
use cadcraft3d::part::work::WorkPlaneDef;
use cadcraft3d::part::{
    BlendFeature, BodyOp, BodyOpFeature, EdgeRef, ExtrudeFeature, Operation, OriginPlane,
    PartDocument, PlaneRef, ProfileRef, SketchOptions, SketchPlacement, WorkDef, WorkFeature,
};
use cadcraft3d::sketch::{Sketch, SplineFit};
use nalgebra::Point2;

fn p2(x: Real, y: Real) -> Point2<Real> {
    Point2::new(x, y)
}

fn volume(p: &PartDocument) -> Real {
    p.exact_bodies
        .0
        .iter()
        .filter(|s| !s.is_open())
        .filter_map(|s| s.volume_at(2e-4))
        .sum()
}

fn plain() -> SketchOptions {
    SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    }
}

fn sketch_on(
    p: &mut PartDocument,
    base: OriginPlane,
    d: Real,
    draw: impl FnOnce(&mut Sketch),
) -> (usize, String) {
    let plane = if d == 0.0 {
        PlaneRef::Origin(base)
    } else {
        let wp = p
            .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
                base: PlaneRef::Origin(base),
                distance: d,
            })))
            .unwrap_or_else(|e| panic!("{e}"));
        PlaneRef::Work(wp)
    };
    let i = p
        .create_sketch(
            SketchPlacement {
                plane,
                ..Default::default()
            },
            &plain(),
        )
        .unwrap_or_else(|e| panic!("{e}"));
    draw(&mut p.sketches[i].sketch);
    (i, p.sketches[i].id.clone())
}

fn extruded(draw: impl FnOnce(&mut Sketch)) -> PartDocument {
    let mut p = PartDocument::default();
    let (i, _) = sketch_on(&mut p, OriginPlane::XY, 0.0, draw);
    let r = p.sketches[i].sketch.regions();
    let ex = ExtrudeFeature::new(vec![ProfileRef::of(&r[0], r[0].interior_point())], 20.0);
    p.add_extrude(i, ex).unwrap_or_else(|e| panic!("{e}"));
    p
}

fn shelled(mut p: PartDocument, z: Real) -> PartDocument {
    let (face, hint) = p
        .body
        .faces()
        .find_map(|(_, f)| {
            let pts = &f.mesh.as_ref()?.positions;
            pts.iter()
                .all(|q| (q.z - z).abs() < 1e-6)
                .then(|| (f.name.clone(), pts[0]))
        })
        .and_then(|(n, q)| Some((n?, q)))
        .unwrap_or_else(|| panic!("no flat face at {z}"));
    let mut sh = ShellFeature::new(2.0);
    sh.remove.push(FaceRef {
        face,
        hint: [hint.x, hint.y, hint.z],
    });
    p.add_shell(sh).unwrap_or_else(|e| panic!("{e}"));
    p
}

fn rounded_outline(sk: &mut Sketch, x: Real, y: Real, w: Real, h: Real, r: Real) {
    let mut pt = |a: Real, b: Real| sk.add_point(p2(x + a, y + b));
    let (a0, a1, b0, b1) = (pt(r, 0.0), pt(w - r, 0.0), pt(w, r), pt(w, h - r));
    let (c0, c1, d0, d1) = (pt(w - r, h), pt(r, h), pt(0.0, h - r), pt(0.0, r));
    let (k1, k2, k3, k4) = (pt(w - r, r), pt(w - r, h - r), pt(r, h - r), pt(r, r));
    for (s, e) in [(a0, a1), (b0, b1), (c0, c1), (d0, d1)] {
        sk.add_line_between(s, e).unwrap_or_else(|e| panic!("{e}"));
    }
    for (c, s, e) in [(k1, a1, b0), (k2, b1, c0), (k3, c1, d0), (k4, d1, a0)] {
        sk.add_arc(c, s, e).unwrap_or_else(|e| panic!("{e}"));
    }
}

fn oval(sk: &mut Sketch, x: Real, y: Real, a: Real, b: Real) {
    let mut pts: Vec<_> = [(a, 0.0), (0.0, b), (-a, 0.0), (0.0, -b), (a, 0.0)]
        .iter()
        .map(|&(u, v)| sk.add_point(p2(x + u, y + v)))
        .collect();
    pts[4] = pts[0];
    let id = sk.add_spline(pts).unwrap_or_else(|e| panic!("{e}"));
    sk.set_spline_fit(
        id,
        SplineFit {
            closed: true,
            ..Default::default()
        },
    )
    .unwrap_or_else(|e| panic!("{e}"));
}

fn rounded() -> PartDocument {
    shelled(
        extruded(|s| rounded_outline(s, 0.0, 0.0, 80.0, 50.0, 8.0)),
        20.0,
    )
}

fn plain_box() -> PartDocument {
    let b = PartDocument::with_box("Box1", [80.0, 50.0, 20.0]).unwrap_or_else(|e| panic!("{e}"));
    shelled(b, 20.0)
}

fn spline_sides() -> PartDocument {
    shelled(
        extruded(|sk| {
            let a = sk.add_point(p2(0.0, 0.0));
            let b = sk.add_point(p2(80.0, 0.0));
            let c = sk.add_point(p2(80.0, 50.0));
            let d = sk.add_point(p2(0.0, 50.0));
            let m1 = sk.add_point(p2(90.0, 25.0));
            let m2 = sk.add_point(p2(-10.0, 25.0));
            sk.add_line_between(a, b).unwrap_or_else(|e| panic!("{e}"));
            sk.add_line_between(c, d).unwrap_or_else(|e| panic!("{e}"));
            sk.add_interp_spline(vec![b, m1, c])
                .unwrap_or_else(|e| panic!("{e}"));
            sk.add_interp_spline(vec![d, m2, a])
                .unwrap_or_else(|e| panic!("{e}"));
        }),
        20.0,
    )
}

/// Upright edges R8, top edges R3, open at the bottom: the ceiling is at z = 18.
fn cover() -> PartDocument {
    let mut b =
        PartDocument::with_box("Box1", [80.0, 50.0, 20.0]).unwrap_or_else(|e| panic!("{e}"));
    let up: Vec<EdgeRef> = b
        .body
        .edges()
        .filter_map(|(e, _)| edge_ref(&b.body, e))
        .filter(|r| r.hint[2] > 1.0 && r.hint[2] < 19.0)
        .collect();
    b.add_blend(BlendFeature::fillet(up, 8.0))
        .unwrap_or_else(|e| panic!("{e}"));
    let top: Vec<EdgeRef> = b
        .body
        .visible_edges()
        .into_iter()
        .filter_map(|e| edge_ref(&b.body, e))
        .filter(|r| (r.hint[2] - 20.0).abs() < 1e-6)
        .collect();
    b.add_blend(BlendFeature::fillet(top, 3.0))
        .unwrap_or_else(|e| panic!("{e}"));
    shelled(b, 0.0)
}

fn attempt(name: &str, mut p: PartDocument, op: BodyOp) -> Real {
    let v = volume(&p);
    p.add_body_op(BodyOpFeature::new(op))
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    assert!(p.exact, "{name}: no longer the exact kernel");
    assert!(
        p.feature_health.is_empty(),
        "{name}: {:?}",
        p.feature_health
    );
    volume(&p) - v
}

fn close(v: Real, want: Real, tol: Real, what: &str) {
    assert!((v - want).abs() < tol, "{what}: {v}, expected {want}");
}

fn boss(
    mut p: PartDocument,
    name: &str,
    base: OriginPlane,
    d: Real,
    at: (Real, Real),
    ribs: bool,
) -> Real {
    let (_, id) = sketch_on(&mut p, base, d, |s| {
        s.add_center_point(p2(at.0, at.1));
    });
    let mut op = BossOp::new(id);
    if ribs {
        op.ribs = Some(BossRibs {
            shoulder: 3.0,
            top_offset: 4.0,
            ..BossRibs::default()
        });
    }
    attempt(name, p, BodyOp::Boss(op))
}

fn rest(
    mut p: PartDocument,
    name: &str,
    base: OriginPlane,
    d: Real,
    draw: impl FnOnce(&mut Sketch),
) -> Real {
    let (_, id) = sketch_on(&mut p, base, d, draw);
    attempt(name, p, BodyOp::Rest(RestOp::new(id, 1.0)))
}

fn grill(
    mut p: PartDocument,
    name: &str,
    base: OriginPlane,
    d: Real,
    draw: impl FnOnce(&mut Sketch),
) -> Real {
    let (_, id) = sketch_on(&mut p, base, d, draw);
    attempt(name, p, BodyOp::Grill(GrillOp::new(id)))
}

fn snap(mut p: PartDocument, name: &str, base: OriginPlane, d: Real, at: (Real, Real)) -> Real {
    let (_, id) = sketch_on(&mut p, base, d, |s| {
        s.add_center_point(p2(at.0, at.1));
    });
    let mut op = SnapFitOp::new(id);
    op.kind = SnapKind::Hook(SnapHook::default());
    attempt(name, p, BodyOp::SnapFit(op))
}

fn fillet_post(mut p: PartDocument, name: &str, rule: FilletRule) -> Real {
    let (i, _) = sketch_on(&mut p, OriginPlane::XY, 2.0, |s| {
        s.add_circle(p2(40.0, 25.0), 3.0)
            .unwrap_or_else(|e| panic!("{e}"));
    });
    let r: Vec<ProfileRef> = p.sketches[i]
        .sketch
        .regions()
        .iter()
        .map(|r| ProfileRef::of(r, r.interior_point()))
        .collect();
    let mut ex = ExtrudeFeature::new(r, 10.0);
    ex.operation = Operation::Join;
    let id = p.add_extrude(i, ex).unwrap_or_else(|e| panic!("{e}"));
    let row = RuleRow::features(vec![id], rule, 1.0);
    attempt(
        name,
        p,
        BodyOp::RuleFillet(RuleFilletOp {
            rows: vec![row],
            exclude_faces: Vec::new(),
            remove_merged: false,
        }),
    )
}

fn rect(x0: Real, y0: Real, x1: Real, y1: Real) -> impl FnOnce(&mut Sketch) {
    move |s| {
        s.add_rectangle_two_point(p2(x0, y0), p2(x1, y1))
            .unwrap_or_else(|e| panic!("{e}"));
    }
}

fn free_edges_of_shell(p: PartDocument, name: &str) -> Real {
    let id = p
        .features
        .iter()
        .find(|f| f.id().starts_with("Shell"))
        .map(|f| f.id().to_owned())
        .unwrap_or_else(|| panic!("no shell"));
    let row = RuleRow::features(vec![id], FilletRule::FreeEdges, 0.5);
    attempt(
        name,
        p,
        BodyOp::RuleFillet(RuleFilletOp {
            rows: vec![row],
            exclude_faces: Vec::new(),
            remove_merged: false,
        }),
    )
}

use OriginPlane::{XY, YZ};

/// A default boss Ø6 with its Ø2.5 hole, per mm of length.
const BOSS_PER_MM: Real = std::f64::consts::PI * (9.0 - 1.5625);

#[test]
fn bosses_on_curved_housings() {
    // From z = 15 to the floor at 2.
    for (name, p, at) in [
        ("rounded, centre", rounded(), (40.0, 25.0)),
        ("rounded, in a corner", rounded(), (9.0, 9.0)),
        ("spline sides", spline_sides(), (40.0, 25.0)),
    ] {
        close(
            boss(p, name, XY, 15.0, at, false),
            BOSS_PER_MM * 13.0,
            0.3,
            name,
        );
    }
    // Tangent to a wall (inside faces at x = 2, corner centre (8, 8) with R6): the post joins it.
    let inner = 8.0 - 3.0 / std::f64::consts::SQRT_2;
    for (name, p, at) in [
        ("tangent to a flat wall", rounded(), (5.0, 25.0)),
        ("tangent inside a corner", rounded(), (inner, inner)),
        ("tangent in a plain box", plain_box(), (5.0, 25.0)),
    ] {
        close(
            boss(p, name, XY, 15.0, at, false),
            BOSS_PER_MM * 13.0,
            0.6,
            name,
        );
    }
    // Up from z = 5 to the ceiling of a cover at 18.
    close(
        boss(cover(), "cover", XY, 5.0, (40.0, 25.0), false),
        BOSS_PER_MM * 13.0,
        0.3,
        "cover",
    );
    // Ribs reaching out towards rounded and spline walls: built, whole.
    let plain = BOSS_PER_MM * 13.0;
    assert!(boss(rounded(), "ribs, corner", XY, 15.0, (12.0, 12.0), true) > plain);
    assert!(boss(spline_sides(), "ribs, spline", XY, 15.0, (82.0, 25.0), true) > plain);
    // Sideways from x = 70 onto the curved walls.
    let d = boss(
        spline_sides(),
        "onto a spline wall",
        YZ,
        70.0,
        (25.0, 10.0),
        false,
    );
    assert!(
        d > BOSS_PER_MM * 16.0 && d < BOSS_PER_MM * 20.0,
        "onto a spline wall: {d}"
    );
    boss(rounded(), "onto a corner", YZ, 70.0, (38.0, 12.0), false);
    boss(
        rounded(),
        "across the tangent",
        YZ,
        70.0,
        (41.0, 12.0),
        false,
    );
}

#[test]
fn rests_and_grills_on_curved_housings() {
    // A 20 x 20 rest over a 2 thick floor; rim 1: the hole 18 x 18, walls up 8 - 2 - 1, landing.
    let want = 76.0 * 5.0 + 324.0 - 324.0 * 2.0 + 76.0;
    close(
        rest(
            rounded(),
            "rest, floor",
            XY,
            8.0,
            rect(30.0, 15.0, 50.0, 35.0),
        ),
        want,
        0.5,
        "rest",
    );
    rest(rounded(), "rest, rounded outline", XY, 8.0, |s| {
        rounded_outline(s, 30.0, 15.0, 20.0, 20.0, 4.0)
    });
    rest(rounded(), "rest, spline outline", XY, 8.0, |s| {
        oval(s, 40.0, 25.0, 30.0, 18.0)
    });
    rest(
        spline_sides(),
        "rest, spline wall",
        YZ,
        100.0,
        rect(18.0, 6.0, 32.0, 14.0),
    );
    rest(
        cover(),
        "rest, over a round",
        XY,
        25.0,
        rect(1.0, 10.0, 12.0, 40.0),
    );
    // A rim 1 wide round an opening 18 x 18 through the 2 mm floor.
    close(
        grill(
            rounded(),
            "grill, floor",
            XY,
            8.0,
            rect(30.0, 15.0, 50.0, 35.0),
        ),
        -648.0,
        0.5,
        "grill",
    );
    grill(rounded(), "grill, rounded outline", XY, 8.0, |s| {
        rounded_outline(s, 30.0, 15.0, 20.0, 20.0, 4.0)
    });
    grill(rounded(), "grill, spline outline", XY, 8.0, |s| {
        oval(s, 40.0, 25.0, 30.0, 18.0)
    });
    // Through a 2 thick curved wall: 12 x 6 of opening.
    close(
        grill(
            spline_sides(),
            "grill, spline wall",
            YZ,
            100.0,
            rect(18.0, 6.0, 32.0, 14.0),
        ),
        -144.0,
        3.0,
        "curved wall",
    );
    grill(
        rounded(),
        "grill, corner wall",
        YZ,
        100.0,
        rect(44.0, 6.0, 49.0, 14.0),
    );
    grill(
        cover(),
        "grill, over a round",
        XY,
        25.0,
        rect(1.0, 10.0, 12.0, 40.0),
    );
}

#[test]
fn snap_fits_on_curved_housings() {
    let plain = snap(rounded(), "floor", XY, 5.0, (40.0, 25.0));
    assert!(plain > 0.0);
    assert!(snap(spline_sides(), "spline wall", YZ, 70.0, (25.0, 10.0)) > plain);
    let mut c = cover();
    let (_, id) = sketch_on(&mut c, XY, 15.0, |s| {
        s.add_center_point(p2(40.0, 25.0));
    });
    let mut op = SnapFitOp::new(id);
    op.kind = SnapKind::Hook(SnapHook::default());
    op.flip = true;
    close(
        attempt("ceiling", c, BodyOp::SnapFit(op)),
        plain,
        0.5,
        "the same hook from a ceiling",
    );
}

/// The fillet at the foot of a concave edge: section (1 - pi/4) r^2.
const FOOT: Real = (1.0 - std::f64::consts::FRAC_PI_4) * 0.25;

/// A shell's own edges on a box with rounded corners: the seams where a flat wall runs on into a
/// corner are tangent and are left out (they refused the whole feature).
#[test]
fn rule_fillets_on_curved_housings() {
    let foot = perimeter_inner() * FOOT;
    close(
        free_edges_of_shell(rounded(), "rounded"),
        foot,
        0.2,
        "free edges of a shell",
    );
    // Inside a cover with rounded edges every edge of the shell is tangent: nothing to round.
    close(
        free_edges_of_shell(cover(), "cover"),
        0.0,
        0.05,
        "a cover's shell",
    );
    assert!(free_edges_of_shell(spline_sides(), "spline sides") > 0.0);
    let ring = fillet_post(rounded(), "post, rounded", FilletRule::AgainstPart);
    assert!(ring > 4.0 && ring < 5.0, "post: {ring}");
    close(
        fillet_post(spline_sides(), "post, spline", FilletRule::AgainstPart),
        ring,
        0.05,
        "same post",
    );
    assert!(fillet_post(spline_sides(), "free, spline", FilletRule::FreeEdges) < 0.0);
}

fn perimeter_inner() -> Real {
    2.0 * (76.0 + 46.0) - 8.0 * 6.0 + 2.0 * std::f64::consts::PI * 6.0
}
