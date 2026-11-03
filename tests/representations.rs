//! Representations of an assembly (spec 3.9) with the real solver: a positional
//! override moves the mechanism and [Primary] comes back, for five kinds of
//! relationship; a model state changes the BOM; everything survives a save.
//! Geometry is given directly, a 20 mm cube with named faces.

use cadcraft3d::assembly::bom::{BomView, ComponentInfo, rows};
use cadcraft3d::assembly::reps::{OccOverride, PRIMARY, RelOverride, Representations};
use cadcraft3d::assembly::{Constraint, ConstraintKind, Geom, GeomRef, solve};
use cadcraft3d::document::{AssemblyDocument, DocRef, DocumentData, DocumentId, Project};
use cadcraft3d::kernel::{Point3, Real, Transform3, Vector3};

fn cube_geom(r: &GeomRef) -> Result<Geom, String> {
    if let GeomRef::Origin { item, .. } = r {
        return Ok(item.geom());
    }
    let GeomRef::Face { name, .. } = r else {
        return Err("unknown".into());
    };
    let (p, n) = match name.as_str() {
        "Top" => (Point3::new(10.0, 10.0, 20.0), Vector3::z()),
        "Bottom" => (Point3::new(10.0, 10.0, 0.0), -Vector3::z()),
        "Right" => (Point3::new(20.0, 10.0, 10.0), Vector3::x()),
        "Left" => (Point3::new(0.0, 10.0, 10.0), -Vector3::x()),
        "Front" => (Point3::new(10.0, 0.0, 10.0), -Vector3::y()),
        "Back" => (Point3::new(10.0, 20.0, 10.0), Vector3::y()),
        _ => return Err(format!("no face {name}")),
    };
    Ok(Geom::Plane { p, n })
}

fn face(o: u32, name: &str) -> GeomRef {
    GeomRef::Face {
        sub: Vec::new(),
        occurrence: o,
        name: name.into(),
        hint: [0.0; 3],
    }
}

fn con(kind: ConstraintKind, a: GeomRef, b: GeomRef, value: Real) -> Constraint {
    Constraint {
        id: 0,
        name: String::new(),
        kind,
        a,
        b,
        value,
        expr: None,
        aligned: false,
        suppressed: false,
        locked: false,
        min: None,
        max: None,
        resting: false,
        plane: None,
    }
}

/// Solves and writes the places; panics when it does not hold.
fn settle(a: &mut AssemblyDocument) {
    let s = solve(a, &cube_geom, None);
    assert!(s.converged, "not solved: {:?}", s.sick);
    for (id, t) in &s.transforms {
        if let Some(o) = a.find_mut(*id) {
            o.transform = *t;
        }
    }
}

fn pair() -> AssemblyDocument {
    let mut a = AssemblyDocument::default();
    let r = DocRef::local(DocumentId::new_random());
    a.place(r, "Cube", Transform3::identity(), true);
    a.place(r, "Cube", Transform3::translation(0.0, 0.0, 30.0), false);
    a
}

fn origin(a: &AssemblyDocument, id: u32) -> Vector3 {
    a.find(id).unwrap().transform.translation.vector
}

/// Activates through the live state, as the GUI does, and solves.
fn activate(r: &mut Representations, a: &mut AssemblyDocument, id: u32) {
    r.sync(a);
    assert!(r.activate_position(id, a));
    settle(a);
    r.sync(a);
}

/// Overrides written into the active position take effect, and are solved.
fn apply(r: &mut Representations, a: &mut AssemblyDocument) {
    r.compose(a);
    settle(a);
    r.sync(a);
}

/// `kind` between faces `fa` of 1 and `fb` of 2 at `value`; an override `to` must move
/// occurrence 2 by `along` · (to − value), and [Primary] must get its place back.
fn overrides_move(kind: ConstraintKind, fa: &str, fb: &str, value: Real, to: Real, along: Vector3) {
    let mut a = pair();
    a.add_constraint(con(kind, face(1, fa), face(2, fb), value));
    let id = a.constraints[0].id;
    settle(&mut a);
    let start = origin(&a, 2);
    let mut r = Representations::default();
    r.new_position(&mut a);
    r.positions[0].relationships.insert(
        id,
        RelOverride {
            value: Some(to),
            suppressed: None,
        },
    );
    apply(&mut r, &mut a);
    let moved = origin(&a, 2) - start;
    assert!(
        (moved - along * (to - value)).norm() < 1e-6,
        "{kind:?}: moved {moved}"
    );
    assert_eq!(a.constraint(id).unwrap().value, to);
    activate(&mut r, &mut a, PRIMARY);
    assert!(
        (origin(&a, 2) - start).norm() < 1e-6,
        "{kind:?}: back at {}",
        origin(&a, 2)
    );
    assert_eq!(a.constraint(id).unwrap().value, value);
    assert!(!r.positions[0].relationships.is_empty(), "kept");
}

#[test]
fn a_mate_override_moves_and_primary_returns() {
    // The offset runs along the first face's outward normal.
    overrides_move(
        ConstraintKind::Mate,
        "Top",
        "Bottom",
        0.0,
        15.0,
        Vector3::z(),
    );
}

#[test]
fn a_flush_override_moves_and_primary_returns() {
    overrides_move(
        ConstraintKind::Flush,
        "Top",
        "Top",
        25.0,
        40.0,
        Vector3::z(),
    );
}

#[test]
fn a_side_mate_override_moves_and_primary_returns() {
    overrides_move(
        ConstraintKind::Mate,
        "Right",
        "Left",
        2.0,
        12.0,
        Vector3::x(),
    );
}

#[test]
fn a_back_mate_override_moves_and_primary_returns() {
    overrides_move(
        ConstraintKind::Mate,
        "Back",
        "Front",
        5.0,
        1.0,
        Vector3::y(),
    );
}

#[test]
fn an_angle_override_turns_and_primary_returns() {
    let mut a = pair();
    // Tops at 30°, then at 60°: the angle between the normals is what is held.
    a.add_constraint(con(
        ConstraintKind::Angle,
        face(1, "Top"),
        face(2, "Top"),
        30f64.to_radians(),
    ));
    let id = a.constraints[0].id;
    settle(&mut a);
    let normal = |a: &AssemblyDocument| a.find(2).unwrap().transform * Vector3::z();
    let start = normal(&a);
    assert!((start.dot(&Vector3::z()) - 30f64.to_radians().cos()).abs() < 1e-5);
    let mut r = Representations::default();
    r.new_position(&mut a);
    r.positions[0].relationships.insert(
        id,
        RelOverride {
            value: Some(60f64.to_radians()),
            suppressed: None,
        },
    );
    apply(&mut r, &mut a);
    assert!((normal(&a).dot(&Vector3::z()) - 60f64.to_radians().cos()).abs() < 1e-5);
    activate(&mut r, &mut a, PRIMARY);
    assert!((normal(&a) - start).norm() < 1e-6);
}

#[test]
fn a_suppress_override_frees_the_component() {
    let mut a = pair();
    a.add_constraint(con(
        ConstraintKind::Mate,
        face(1, "Top"),
        face(2, "Bottom"),
        0.0,
    ));
    let id = a.constraints[0].id;
    settle(&mut a);
    let mut r = Representations::default();
    let p = r.new_position(&mut a);
    r.positions[0].relationships.insert(
        id,
        RelOverride {
            value: None,
            suppressed: Some(true),
        },
    );
    apply(&mut r, &mut a);
    assert!(a.constraint(id).unwrap().suppressed);
    // A free component stays where it is put, in that position only.
    let here = Transform3::translation(100.0, 0.0, 0.0);
    a.find_mut(2).unwrap().transform = here;
    r.sync(&a);
    activate(&mut r, &mut a, PRIMARY);
    assert!(!a.constraint(id).unwrap().suppressed);
    assert!((origin(&a, 2) - Vector3::new(0.0, 0.0, 20.0)).norm() < 1e-6);
    activate(&mut r, &mut a, p);
    assert_eq!(a.find(2).unwrap().transform, here);
}

#[test]
fn a_grounded_offset_override_holds_against_the_relationships() {
    let mut a = pair();
    a.add_constraint(con(
        ConstraintKind::Mate,
        face(1, "Top"),
        face(2, "Bottom"),
        0.0,
    ));
    settle(&mut a);
    let mut r = Representations::default();
    r.new_position(&mut a);
    // Grounded elsewhere, and the mate suppressed so that it can hold.
    let id = a.constraints[0].id;
    r.positions[0].relationships.insert(
        id,
        RelOverride {
            value: None,
            suppressed: Some(true),
        },
    );
    r.positions[0].occurrences.insert(
        2,
        OccOverride {
            grounded: Some(true),
            offset: Some([5.0, 6.0, 7.0]),
        },
    );
    apply(&mut r, &mut a);
    assert!(a.find(2).unwrap().grounded);
    assert_eq!(origin(&a, 2), Vector3::new(5.0, 6.0, 7.0));
    activate(&mut r, &mut a, PRIMARY);
    assert!(!a.find(2).unwrap().grounded);
    assert!((origin(&a, 2) - Vector3::new(0.0, 0.0, 20.0)).norm() < 1e-6);
}

#[test]
fn a_position_that_conflicts_does_not_solve() {
    let mut a = pair();
    a.add_constraint(con(
        ConstraintKind::Mate,
        face(1, "Top"),
        face(2, "Bottom"),
        0.0,
    ));
    a.add_constraint(con(
        ConstraintKind::Flush,
        face(1, "Top"),
        face(2, "Top"),
        20.0,
    ));
    settle(&mut a);
    let mut r = Representations::default();
    r.new_position(&mut a);
    let id = a.constraints[0].id;
    a.constraint_mut(id).unwrap().value = 50.0;
    r.sync(&a);
    let c = r.composed(&a, None, None, None);
    assert!(!solve(&c, &cube_geom, None).converged);
}

#[test]
fn a_model_state_changes_the_bom_and_primary_keeps_its_own() {
    let mut a = AssemblyDocument::default();
    let bolt = DocRef::local(DocumentId::new_random());
    let plate = DocRef::local(DocumentId::new_random());
    a.place(plate, "Plate", Transform3::identity(), true);
    a.place(bolt, "Bolt", Transform3::identity(), false);
    a.place(bolt, "Bolt", Transform3::identity(), false);
    let info = |r: DocRef| ComponentInfo {
        file_name: if r == bolt { "BOLT" } else { "PLATE" }.into(),
        ..Default::default()
    };
    let qty = |a: &AssemblyDocument, n: &str| {
        rows(a, BomView::Structured, true, &info)
            .iter()
            .find(|r| r.part_number == n)
            .map_or(0, |r| r.qty)
    };
    assert_eq!(qty(&a, "BOLT"), 2);
    let mut r = Representations::default();
    let s = r.new_state(&a);
    a.find_mut(3).unwrap().suppressed = true;
    r.sync(&a);
    assert_eq!(qty(&a, "BOLT"), 1);
    r.activate_state(PRIMARY, &mut a);
    assert_eq!(qty(&a, "BOLT"), 2);
    r.activate_state(s, &mut a);
    assert_eq!(qty(&a, "BOLT"), 1);
    assert_eq!(qty(&a, "PLATE"), 1);
}

#[test]
fn everything_survives_a_save_and_load() {
    let mut p = Project::new("p");
    let block = p.add_document(
        "Block",
        DocumentData::Part(cadcraft3d::part::PartDocument::with_box("Box1", [20.0; 3]).unwrap()),
    );
    let mut a = pair();
    for o in &mut a.occurrences {
        o.component = DocRef::local(block);
    }
    a.add_constraint(con(
        ConstraintKind::Mate,
        face(1, "Top"),
        face(2, "Bottom"),
        0.0,
    ));
    let mut r = Representations::default();
    let view = r.new_view(&a);
    a.find_mut(2).unwrap().transparent = true;
    r.sync(&a);
    let state = r.new_state(&a);
    let position = r.new_position(&mut a);
    a.constraints[0].value = 12.0;
    r.sync(&a);
    a.reps = r;
    let id = p.add_document("A", DocumentData::Assembly(a.clone()));
    let dir = std::env::temp_dir().join(format!("asm-reps-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("a.3dcadcraft");
    p.save(&path).unwrap();
    let q = Project::load(&path).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let Some(DocumentData::Assembly(b)) = q.document(id).map(|d| &d.data) else {
        panic!("no assembly");
    };
    assert_eq!(b.reps, a.reps);
    assert_eq!(
        (
            b.reps.active_view,
            b.reps.active_state,
            b.reps.active_position
        ),
        (view, state, position)
    );
    assert_eq!(b.constraints[0].value, 12.0);
    assert_eq!(
        b.reps.positions[0].relationships[&b.constraints[0].id].value,
        Some(12.0)
    );
}
