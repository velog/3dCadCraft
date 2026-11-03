//! A thin-walled housing with one sketch ready for each plastic command, to try them in the app:
//! `cargo run --example plastic_demo -- out.3dcadcraft [built]`. With `built` the features are
//! already there.

use cadcraft3d::document::Project;
use cadcraft3d::document::DocumentData;
use cadcraft3d::part::blend::edge_ref;
use cadcraft3d::part::plastic::boss::{BossHead, BossKind, BossOp, BossRibs};
use cadcraft3d::part::plastic::grill::{GrillCurves, GrillOp};
use cadcraft3d::part::plastic::lip::LipOp;
use cadcraft3d::part::plastic::rule_fillet::{FilletRule, RuleFilletOp, RuleRow};
use cadcraft3d::part::plastic::snap_fit::SnapFitOp;
use cadcraft3d::part::work::{LineRef, WorkPlaneDef};
use cadcraft3d::part::{
    BodyOp, BodyOpFeature, ExtrudeFeature, OriginPlane, Operation, PartDocument, PlaneRef,
    ProfileRef, SketchOptions, SketchPlacement, WorkDef, WorkFeature,
};
use nalgebra::Point2;

type R<T> = Result<T, Box<dyn std::error::Error>>;

fn plain() -> SketchOptions {
    SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    }
}

fn at_z(p: &mut PartDocument, z: f64) -> R<usize> {
    let wp = p.add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
        base: PlaneRef::Origin(OriginPlane::XY),
        distance: z,
    })))?;
    Ok(p.create_sketch(
        SketchPlacement {
            plane: PlaneRef::Work(wp),
            ..Default::default()
        },
        &plain(),
    )?)
}

/// A housing 120 x 80 x 30: floor 2 thick at z from 0 to 2, walls 2 thick, open at the top.
fn housing() -> R<PartDocument> {
    let mut p = PartDocument::default();
    let i = p.create_sketch(
        SketchPlacement {
            plane: PlaneRef::Origin(OriginPlane::XY),
            ..Default::default()
        },
        &plain(),
    )?;
    let sk = &mut p.sketches[i].sketch;
    sk.add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(120.0, 80.0))?;
    let r: Vec<ProfileRef> = sk
        .regions()
        .iter()
        .map(|r| ProfileRef::of(r, r.interior_point()))
        .collect();
    let mut ex = ExtrudeFeature::new(r, 30.0);
    ex.operation = Operation::NewSolid;
    p.add_extrude(i, ex)?;
    let j = at_z(&mut p, 2.0)?;
    let sk = &mut p.sketches[j].sketch;
    sk.add_rectangle_two_point(Point2::new(2.0, 2.0), Point2::new(118.0, 78.0))?;
    let r: Vec<ProfileRef> = sk
        .regions()
        .iter()
        .map(|r| ProfileRef::of(r, r.interior_point()))
        .collect();
    let mut cut = ExtrudeFeature::new(r, 40.0);
    cut.operation = Operation::Cut;
    p.add_extrude(j, cut)?;
    Ok(p)
}

fn main() -> R<()> {
    let args: Vec<String> = std::env::args().collect();
    let out = args.get(1).ok_or("usage: plastic_demo out.3dcadcraft [built]")?;
    let built = args.get(2).is_some_and(|a| a == "built");
    let mut p = housing()?;
    // The rim's inner edges, before anything else stands on the rim.
    let rim: Vec<_> = {
        let b = &p.body;
        b.visible_edges()
            .into_iter()
            .filter_map(|e| {
                let pts = b.sample_edge(e, &Default::default()).ok()?;
                let top = pts.iter().all(|q| (q.z - 30.0).abs() < 1e-6);
                let inner = pts.iter().all(|q| {
                    (1.0..119.0).contains(&q.x) && (1.0..79.0).contains(&q.y)
                });
                (top && inner).then(|| edge_ref(b, e)).flatten()
            })
            .collect()
    };
    // Grill: boundary, ribs and spar, seen on the floor from below.
    let g = at_z(&mut p, -5.0)?;
    let sk = &mut p.sketches[g].sketch;
    sk.add_rectangle_two_point(Point2::new(10.0, 10.0), Point2::new(60.0, 40.0))?;
    let ribs: Vec<_> = [20.0, 30.0, 40.0, 50.0]
        .iter()
        .map(|x| sk.add_line(Point2::new(*x, 5.0), Point2::new(*x, 45.0)))
        .collect();
    let spar = vec![sk.add_line(Point2::new(5.0, 25.0), Point2::new(65.0, 25.0))];
    let grill_id = p.sketches[g].id.clone();
    // Boss: thread posts on the floor, head boss under them.
    let b = at_z(&mut p, 25.0)?;
    p.sketches[b].sketch.add_center_point(Point2::new(75.0, 20.0));
    p.sketches[b].sketch.add_center_point(Point2::new(105.0, 20.0));
    let boss_id = p.sketches[b].id.clone();
    // Snap fit on the floor.
    let s = at_z(&mut p, 2.0)?;
    p.sketches[s].sketch.add_center_point(Point2::new(85.0, 60.0));
    let snap_id = p.sketches[s].id.clone();
    // Rest: a pad on the floor.
    let r = at_z(&mut p, 8.0)?;
    p.sketches[r]
        .sketch
        .add_rectangle_two_point(Point2::new(20.0, 52.0), Point2::new(50.0, 72.0))?;
    if built {
        let mut op = GrillOp::new(grill_id.clone());
        op.outside_height = 0.5;
        op.ribs = Some(GrillCurves {
            sketch: grill_id.clone(),
            curves: ribs,
            thickness: 1.5,
            height: 2.0,
            top_offset: 0.0,
            bottom_offset: 0.0,
        });
        op.spars = Some(GrillCurves {
            sketch: grill_id,
            curves: spar,
            thickness: 2.0,
            height: 0.0,
            top_offset: 0.5,
            bottom_offset: 1.0,
        });
        println!("adding Grill");
        p.add_body_op(BodyOpFeature::new(BodyOp::Grill(op)))?;
        let mut op = BossOp::new(boss_id);
        op.ribs = Some(BossRibs {
            shoulder: 3.0,
            top_offset: 8.0,
            ..BossRibs::default()
        });
        op.fillet = 0.5;
        println!("adding Boss");
        let boss = p.add_body_op(BodyOpFeature::new(BodyOp::Boss(op)))?;
        let _ = (BossKind::Head(BossHead::default()), boss);
        println!("adding SnapFit");
        p.add_body_op(BodyOpFeature::new(BodyOp::SnapFit(SnapFitOp::new(snap_id))))?;
        let mut op = cadcraft3d::part::plastic::rest::RestOp::new(p.sketches[r].id.clone(), 1.5);
        op.flip = false;
        println!("adding Rest");
        p.add_body_op(BodyOpFeature::new(BodyOp::Rest(op)))?;
        let mut lip = LipOp::new(rim, 1.5, 1.0);
        lip.pull = Some(LineRef::Origin(2));
        println!("adding Lip");
        let lip_id = p.add_body_op(BodyOpFeature::new(BodyOp::Lip(lip)))?;
        let row = RuleRow::features(vec![lip_id], FilletRule::FreeEdges, 0.3);
        println!("adding RuleFillet");
        p.add_body_op(BodyOpFeature::new(BodyOp::RuleFillet(RuleFilletOp {
            rows: vec![row],
            exclude_faces: Vec::new(),
            remove_merged: false,
        })))?;
    }
    for ps in &p.sketches {
        println!("{} visible {}", ps.id, ps.visible);
    }
    println!(
        "features {} health {:?}",
        p.features.len(),
        p.feature_health
    );
    let mut project = Project::new("Housing");
    project.add_document("Housing", DocumentData::Part(p));
    project.save(std::path::Path::new(out))?;
    Ok(())
}
