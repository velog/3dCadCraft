//! Sheet metal Cut extents and Direction (`docs/spec/5.6-sheet-metal.md`
//! §6.1h): a U channel (base 100 × 60, T 2, walls 40 high on its long edges)
//! cut by a 20 × 10 rectangle sketched on planes parallel to the walls.
//! Inventor 2027 on the same channel: Through All cuts every wall on its
//! side, To Next only the nearest, a Distance that ends inside a wall leaves
//! a pocket, To a plane inside a wall cuts up to it; each wall cut out takes
//! 20 · 10 · 2 = 400 mm³.

use cadcraft3d::kernel::{Point3, Real, Surface};
use cadcraft3d::part::sheet_metal::{
    CutDirection, CutExtent, SheetFlangeFeature, SheetMetalDefaults, SheetOp, SheetOpFeature,
};
use cadcraft3d::part::{
    OriginPlane, PartDocument, PlaneRef, ProfileRef, SketchOptions, SketchPlacement, WorkDef,
    WorkFeature, WorkPlaneDef,
};
use cadcraft3d::sketch::Point2;

fn opts() -> SketchOptions {
    SketchOptions {
        auto_project_origin: false,
        ..Default::default()
    }
}

fn channel() -> PartDocument {
    let mut p = PartDocument::default();
    p.make_sheet_metal(Some(SheetMetalDefaults {
        thickness: 2.0,
        ..Default::default()
    }));
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    p.sketches[i]
        .sketch
        .add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(100.0, 60.0))
        .unwrap();
    let r = p.sketches[i].sketch.regions();
    p.add_sheet_face(i, vec![ProfileRef::of(&r[0], r[0].interior_point())], false)
        .unwrap();
    let mut edges = Vec::new();
    let mut body = String::new();
    for y in [0.0, 60.0] {
        let (b, e) = p.sheet_edge_at(Point3::new(50.0, y, 2.0)).unwrap();
        body = b;
        edges.push(e);
    }
    p.add_sheet_flange(SheetFlangeFeature::new(&body, edges, 40.0))
        .unwrap();
    p
}

/// The two walls' (lowest, highest) y.
fn walls(p: &PartDocument) -> [(Real, Real); 2] {
    let mut ys: Vec<Real> = p
        .body
        .faces()
        .filter_map(|(_, f)| match p.body.surface(f.surface) {
            Some(Surface::Plane(pl)) if pl.normal.y.abs() > 1.0 - 1e-9 => Some(pl.origin.y),
            _ => None,
        })
        .collect();
    ys.sort_by(|a, b| a.total_cmp(b));
    ys.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    // Four wall faces: two of each wall.
    assert!(ys.len() >= 4, "{ys:?}");
    let n = ys.len();
    [(ys[0], ys[1]), (ys[n - 2], ys[n - 1])]
}

/// The channel cut by the rectangle x 40..60, z 15..25 on the plane y = `y`.
fn cut(
    y: Real,
    extent: CutExtent,
    depth: Real,
    direction: CutDirection,
    to: Option<Real>,
) -> Result<PartDocument, String> {
    let mut p = channel();
    // The XZ plane's normal is −Y: an offset d lies at y = −d.
    let plane = |p: &mut PartDocument, y: Real| {
        p.add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
            base: PlaneRef::Origin(OriginPlane::XZ),
            distance: -y,
        })))
        .unwrap()
    };
    let wp = plane(&mut p, y);
    let target = to.map(|t| PlaneRef::Work(plane(&mut p, t)));
    let i = p
        .create_sketch(
            SketchPlacement {
                plane: PlaneRef::Work(wp),
                ..Default::default()
            },
            &opts(),
        )
        .unwrap();
    let sk = &mut p.sketches[i];
    sk.sketch
        .add_rectangle_two_point(Point2::new(40.0, 15.0), Point2::new(60.0, 25.0))
        .unwrap();
    let id = sk.id.clone();
    let r = p.sketches[i].sketch.regions();
    p.add_sheet_op(SheetOpFeature::new(SheetOp::Cut {
        sketch: id,
        profiles: vec![ProfileRef::of(&r[0], r[0].interior_point())],
        extent,
        depth,
        across: false,
        direction: Some(direction),
        to: target,
    }))
    .map_err(|e| e.to_string())?;
    Ok(p)
}

fn volume(p: &PartDocument) -> Real {
    p.exact_bodies.0[0].volume_at(1e-3).unwrap()
}

#[test]
fn extents_and_directions_as_inventor() {
    let v0 = volume(&channel());
    let [w1, w2] = walls(&channel());
    let outside = w1.0 - 10.0;
    // Nearer the first wall, between the two.
    let between = w1.1 + 23.0;
    use CutDirection::{Default as D, Flipped as F, Symmetric as S};
    use CutExtent::{Distance, ThroughAll, To, ToNext};
    // (what, plane y, extent, depth, direction, to y, removed volume)
    type Case = (
        &'static str,
        Real,
        CutExtent,
        Real,
        CutDirection,
        Option<Real>,
        Real,
    );
    let cases: Vec<Case> = vec![
        (
            "through all from outside",
            outside,
            ThroughAll,
            0.0,
            D,
            None,
            800.0,
        ),
        ("to next from outside", outside, ToNext, 0.0, D, None, 400.0),
        (
            "distance 12 from outside",
            outside,
            Distance,
            12.0,
            D,
            None,
            400.0,
        ),
        (
            "distance 11 from outside: a pocket",
            outside,
            Distance,
            11.0,
            D,
            None,
            200.0,
        ),
        (
            "through all between",
            between,
            ThroughAll,
            0.0,
            D,
            None,
            400.0,
        ),
        (
            "through all between flipped",
            between,
            ThroughAll,
            0.0,
            F,
            None,
            400.0,
        ),
        (
            "through all between symmetric",
            between,
            ThroughAll,
            0.0,
            S,
            None,
            800.0,
        ),
        ("to next between", between, ToNext, 0.0, D, None, 400.0),
        (
            "to next between flipped",
            between,
            ToNext,
            0.0,
            F,
            None,
            400.0,
        ),
        (
            "to next between symmetric",
            between,
            ToNext,
            0.0,
            S,
            None,
            800.0,
        ),
        (
            "to a plane inside the first wall",
            between,
            To,
            0.0,
            D,
            Some(w1.0 + 1.0),
            200.0,
        ),
        (
            "to a plane past the far wall",
            outside,
            To,
            0.0,
            D,
            Some(w2.1 + 1.0),
            800.0,
        ),
    ];
    let mut bad = Vec::new();
    for (what, y, extent, depth, dir, to, removed) in cases {
        match cut(y, extent, depth, dir, to) {
            Ok(p) if p.feature_health.is_empty() => {
                let got = v0 - volume(&p);
                if (got - removed).abs() > 0.5 {
                    bad.push(format!("{what}: removed {got:.2}, want {removed}"));
                }
            }
            Ok(p) => bad.push(format!("{what}: {:?}", p.feature_health)),
            Err(e) => bad.push(format!("{what}: {e}")),
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
    // The flipped cuts went to the other wall: its faces at x = 40..60.
    let p = cut(between, ThroughAll, 0.0, F, None).unwrap();
    let far_cut = p.body.faces().any(|(_, f)| {
        matches!(p.body.surface(f.surface), Some(Surface::Plane(pl))
            if pl.normal.x.abs() > 1.0 - 1e-9 && (pl.origin.x - 40.0).abs() < 1e-6)
            && f.outer.coedges.iter().any(|c| {
                p.body
                    .edge(c.edge)
                    .and_then(|e| p.body.vertex(e.start))
                    .is_some_and(|v| v.point.y > w2.0 - 1e-6)
            })
    });
    assert!(far_cut, "the flipped Through All did not cut the far wall");
}

#[test]
fn a_cut_meeting_nothing_is_refused() {
    let [w1, _] = walls(&channel());
    // 5 mm from 10 mm outside: short of the wall.
    let r = cut(
        w1.0 - 10.0,
        CutExtent::Distance,
        5.0,
        CutDirection::Default,
        None,
    );
    assert!(
        r.as_ref().is_err() || r.as_ref().is_ok_and(|p| !p.feature_health.is_empty()),
        "a cut that meets no wall must fail"
    );
}
