//! Dragging chains of joints (`docs/spec/3.1-assemblies.md` §3.3, drag) and
//! dragging inside a flexible subassembly (`docs/spec/3.1a-subassemblies.md`
//! S5): an arm turning on a base with a slide along it and a second arm on
//! the slide, and a delta robot (three hinged arms, a rod on Balls from each
//! tip to one effector).

use cadcraft3d::assembly::flexible;
use cadcraft3d::assembly::mechanisms::{self, ARM_TIP, DELTA, EFFECTOR_CENTRE};
use cadcraft3d::assembly::solve::{Drag, Solved};
use cadcraft3d::document::{AssemblyDocument, DocRef, DocumentData, DocumentId, Project};
use cadcraft3d::kernel::{Point3, Real, Transform3, Vector3};

fn asm(p: &Project, id: DocumentId) -> AssemblyDocument {
    match &p.document(id).unwrap().data {
        DocumentData::Assembly(a) => a.clone(),
        _ => panic!("not an assembly"),
    }
}

fn write(s: &Solved, a: &mut AssemblyDocument) {
    flexible::apply(s, a);
}

/// Drags point `local` of `occ` (inside `inside` when given) towards
/// `target` in `steps` cursor moves, as the app does (one solve per move,
/// written back each time).
fn drag_to(
    p: &Project,
    a: &mut AssemblyDocument,
    occ: u32,
    inside: Option<u32>,
    local: Point3,
    target: Point3,
    steps: usize,
) -> Solved {
    let start = world(p, a, occ, inside) * local;
    let mut last = None;
    for k in 1..=steps {
        let f = k as Real / steps as Real;
        let drag = Drag {
            occurrence: occ,
            inside,
            local,
            target: start + (target - start) * f,
        };
        let s = mechanisms::solve_in(p, a, Some(&drag));
        assert!(s.converged, "step {k}: residual {}", s.residual);
        write(&s, a);
        last = Some(s);
    }
    last.unwrap()
}

fn world(p: &Project, a: &AssemblyDocument, occ: u32, inside: Option<u32>) -> Transform3 {
    let sub: Vec<u32> = inside.into_iter().collect();
    mechanisms::placement(p, a, occ, &sub).unwrap()
}

#[test]
fn the_arm_is_put_together_on_its_joints() {
    let mut p = Project::new("T");
    let ids = mechanisms::arm(&mut p).unwrap();
    let a = asm(&p, ids.doc);
    let s = mechanisms::solve_in(&p, &a, None);
    assert!(s.converged, "residual {}", s.residual);
    // Nothing had to move: the parts are placed on their joints.
    for (id, t) in &s.transforms {
        let o = a.find(*id).unwrap().transform;
        assert!(
            (o.inverse() * t).translation.vector.norm() < 1e-6,
            "{id} moved"
        );
    }
    // Degrees of freedom count with the others held: only the last arm can
    // still turn alone (the drag test shows all three motions together).
    assert_eq!(s.dof[&ids.arm2], 1, "{:?}", s.dof);
    assert_eq!(s.dof[&ids.arm1], 0, "{:?}", s.dof);
}

#[test]
fn dragging_the_arm_tip_reaches_through_turn_slide_turn() {
    let mut p = Project::new("T");
    let ids = mechanisms::arm(&mut p).unwrap();
    let mut a = asm(&p, ids.doc);
    // The tip starts at (114, 0, 44); pull it round to the other side and
    // further out: the base turns, the slide runs out, the second arm turns.
    let target = Point3::new(-40.0, 120.0, 44.0);
    drag_to(&p, &mut a, ids.arm2, None, ARM_TIP, target, 40);
    let tip = world(&p, &a, ids.arm2, None) * ARM_TIP;
    assert!((tip - target).norm() < 0.5, "tip at {tip}");
    // The joints still hold, solved without the drag.
    let s = mechanisms::solve_in(&p, &a, None);
    assert!(s.converged, "residual {}", s.residual);
    // The slide stayed on the first arm's top, square to it.
    let arm1 = world(&p, &a, ids.arm1, None);
    let slide = world(&p, &a, ids.slide, None);
    let rel = arm1.inverse() * slide;
    let v = rel.translation.vector;
    assert!((v.y - 2.0).abs() < 1e-4 && (v.z - 10.0).abs() < 1e-4, "{v}");
    assert!(
        rel.rotation.angle() < 1e-4,
        "turned {}",
        rel.rotation.angle()
    );
}

/// Each rod's ends on its arm's tip and on the effector: the rod keeps its
/// length (Balls hold) and each arm turns only about its hinge.
fn delta_holds(p: &Project, a: &AssemblyDocument, ids: &mechanisms::DeltaIds, holder: Option<u32>) {
    let (r_base, l_arm, l_rod, _) = DELTA;
    let at = |o: u32| match holder {
        Some(h) => mechanisms::placement(p, a, h, &[o]).unwrap(),
        None => mechanisms::placement(p, a, o, &[]).unwrap(),
    };
    for k in 0..3 {
        let arm = at(ids.arms[k]);
        let rod = at(ids.rods[k]);
        let tip = arm * Point3::new(l_arm, 5.0, 5.0);
        let top = rod * Point3::new(3.0, 3.0, l_rod);
        assert!(
            (tip - top).norm() < 1e-4,
            "rod {k} off the arm: {}",
            (tip - top).norm()
        );
        let hinge = arm * Point3::new(0.0, 5.0, 5.0);
        let f = (k as Real * 120.0).to_radians();
        let want =
            at(ids.base) * Point3::new(80.0 + r_base * f.cos(), 80.0 + r_base * f.sin(), 0.0);
        assert!((hinge - want).norm() < 1e-4, "arm {k} left its hinge");
    }
}

#[test]
fn the_delta_robot_is_put_together() {
    let mut p = Project::new("T");
    let ids = mechanisms::delta(&mut p).unwrap();
    let a = asm(&p, ids.doc);
    let s = mechanisms::solve_in(&p, &a, None);
    assert!(s.converged, "residual {}", s.residual);
    assert!(
        s.transforms
            .iter()
            .all(|(_, t)| t.translation.vector.norm().is_finite())
    );
    delta_holds(&p, &a, &ids, None);
}

#[test]
fn dragging_the_effector_moves_the_three_arms() {
    let mut p = Project::new("T");
    let ids = mechanisms::delta(&mut p).unwrap();
    let mut a = asm(&p, ids.doc);
    let angle = |a: &AssemblyDocument, k: usize| {
        let t = world(&p, a, ids.arms[k], None);
        let x = t.rotation * Vector3::x();
        x.z.atan2(x.xy().norm())
    };
    let before: Vec<Real> = (0..3).map(|k| angle(&a, k)).collect();
    let start = world(&p, &a, ids.effector, None) * EFFECTOR_CENTRE;
    // Down 25 and 15 towards the first arm.
    let target = start + Vector3::new(15.0, 0.0, -25.0);
    drag_to(&p, &mut a, ids.effector, None, EFFECTOR_CENTRE, target, 25);
    let now = world(&p, &a, ids.effector, None) * EFFECTOR_CENTRE;
    assert!(
        (now - target).norm() < 0.5,
        "effector at {now}, wanted {target}"
    );
    delta_holds(&p, &a, &ids, None);
    // Down: every arm swung down; towards arm 0: it swung less than the
    // others.
    let after: Vec<Real> = (0..3).map(|k| angle(&a, k)).collect();
    for k in 0..3 {
        assert!(
            after[k] < before[k] - 1e-3,
            "arm {k}: {} → {}",
            before[k],
            after[k]
        );
    }
    assert!(after[0] > after[1] && after[0] > after[2], "{after:?}");
}

#[test]
fn dragging_an_arm_carries_the_effector() {
    let mut p = Project::new("T");
    let ids = mechanisms::delta(&mut p).unwrap();
    let mut a = asm(&p, ids.doc);
    let start = world(&p, &a, ids.effector, None) * EFFECTOR_CENTRE;
    // Arm 0's tip 20 mm down.
    let tip = Point3::new(DELTA.1, 5.0, 5.0);
    let from = world(&p, &a, ids.arms[0], None) * tip;
    drag_to(
        &p,
        &mut a,
        ids.arms[0],
        None,
        tip,
        from + Vector3::new(0.0, 0.0, -20.0),
        20,
    );
    delta_holds(&p, &a, &ids, None);
    let now = world(&p, &a, ids.effector, None) * EFFECTOR_CENTRE;
    assert!((now - start).norm() > 3.0, "the effector stayed at {now}");
}

/// Top: the delta robot placed as a flexible subassembly and the arm placed
/// rigid beside it.
fn top(p: &mut Project) -> (AssemblyDocument, mechanisms::DeltaIds, u32, u32) {
    let ids = mechanisms::delta(p).unwrap();
    let arm = mechanisms::arm(p).unwrap();
    let mut top = AssemblyDocument::default();
    let d = top.place(
        DocRef::local(ids.doc),
        "Delta",
        Transform3::translation(0.0, 0.0, 300.0),
        true,
    );
    let r = top.place(
        DocRef::local(arm.doc),
        "Arm",
        Transform3::translation(300.0, 0.0, 0.0),
        true,
    );
    top.find_mut(d).unwrap().flexible = true;
    (top, ids, d, r)
}

#[test]
fn dragging_inside_a_flexible_subassembly() {
    let mut p = Project::new("T");
    let (mut a, ids, d, _) = top(&mut p);
    let sub_before = asm(&p, ids.doc);
    let start = world(&p, &a, d, Some(ids.effector)) * EFFECTOR_CENTRE;
    let target = start + Vector3::new(0.0, 12.0, -20.0);
    let s = drag_to(
        &p,
        &mut a,
        d,
        Some(ids.effector),
        EFFECTOR_CENTRE,
        target,
        25,
    );
    // Only places inside the occurrence changed: the holder stays.
    assert!(s.transforms.iter().all(|(id, _)| *id != d));
    let now = world(&p, &a, d, Some(ids.effector)) * EFFECTOR_CENTRE;
    assert!(
        (now - target).norm() < 0.5,
        "effector at {now}, wanted {target}"
    );
    delta_holds(&p, &a, &ids, Some(d));
    // The subassembly itself is untouched.
    assert_eq!(
        serde_json::to_string(&asm(&p, ids.doc)).unwrap(),
        serde_json::to_string(&sub_before).unwrap()
    );
}

#[test]
fn a_drag_inside_a_rigid_subassembly_moves_nothing_inside() {
    let mut p = Project::new("T");
    let (mut a, ids, d, _) = top(&mut p);
    a.find_mut(d).unwrap().flexible = false;
    let start = world(&p, &a, d, Some(ids.effector)) * EFFECTOR_CENTRE;
    let drag = Drag {
        occurrence: d,
        inside: Some(ids.effector),
        local: EFFECTOR_CENTRE,
        target: start + Vector3::new(0.0, 0.0, -20.0),
    };
    let s = mechanisms::solve_in(&p, &a, Some(&drag));
    assert!(s.flex.is_empty());
    assert!(s.transforms.iter().all(|(id, _)| *id != d));
}

#[test]
fn joints_on_work_features_hold_after_a_save() {
    // Work geometry is not saved: a part read from a file is rebuilt at
    // open (`Project::rebuild_unmeshed_parts`), or every joint on a UCS
    // would find nothing and the drag would move no part.
    let mut p = Project::new("T");
    let (a, ids, d, _) = top(&mut p);
    p.add_document("Top", DocumentData::Assembly(a));
    let dir = std::env::temp_dir().join(format!("asm_mech_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("mech.3dcadcraft");
    p.save(&path).unwrap();
    let mut q = Project::load(&path).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(q.rebuild_unmeshed_parts() >= 4);
    let top_id = q.documents().iter().find(|x| x.name == "Top").unwrap().id;
    let mut a = asm(&q, top_id);
    let start = world(&q, &a, d, Some(ids.effector)) * EFFECTOR_CENTRE;
    let target = start + Vector3::new(0.0, 0.0, -15.0);
    let s = drag_to(
        &q,
        &mut a,
        d,
        Some(ids.effector),
        EFFECTOR_CENTRE,
        target,
        10,
    );
    assert!(s.sick.is_empty(), "{:?}", s.sick);
    let now = world(&q, &a, d, Some(ids.effector)) * EFFECTOR_CENTRE;
    assert!((now - target).norm() < 0.5, "effector at {now}");
}
