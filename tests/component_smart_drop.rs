//! Smart Drop's choices (spec 6.3 §3.5): which size suits a hole or a shaft,
//! and where it lands. Real cases: clearance holes, tapped holes, blind holes,
//! counterbores' bores, shafts, housings.

use cadcraft3d::components::catalog::Catalog;
use cadcraft3d::components::fit::{Fit, Target, choose, goes_in, seat_transform};
use cadcraft3d::components::samples::{self, Sample};
use cadcraft3d::kernel::{Point3, Vector3};
use cadcraft3d::part::connection::{Connection, ConnectionRole, next_name};

fn family(name: &str) -> Vec<Sample> {
    let all: Vec<Sample> = samples::all()
        .into_iter()
        .filter(|s| s.family == name)
        .collect();
    assert!(!all.is_empty(), "no family {name}");
    all
}

fn hole(diameter: f64, depth: f64) -> Target {
    Target {
        hole: true,
        diameter,
        thread: None,
        depth,
        blind: false,
    }
}

fn shaft(diameter: f64) -> Target {
    Target {
        hole: false,
        diameter,
        thread: None,
        depth: 40.0,
        blind: false,
    }
}

/// The size `family` gets on `t`.
fn dropped(name: &str, t: &Target) -> Option<String> {
    let members = family(name);
    let fits: Vec<Fit> = members.iter().map(|m| Fit::of(&m.kind)).collect();
    choose(&fits, t).map(|i| members[i].size.clone())
}

#[test]
fn a_clearance_hole_takes_the_widest_screw_that_passes_and_the_shortest_length() {
    // ISO 273 medium clearance for M8 is 9, for M10 is 11.
    assert_eq!(
        dropped("ISO 4017", &hole(9.0, 10.0)).as_deref(),
        Some("M8 x 16")
    );
    assert_eq!(
        dropped("ISO 4017", &hole(11.0, 12.0)).as_deref(),
        Some("M10 x 20")
    );
    // An exact hole still takes its screw.
    assert_eq!(
        dropped("ISO 4762", &hole(6.0, 8.0)).as_deref(),
        Some("M6 x 12")
    );
}

#[test]
fn a_tapped_hole_takes_its_thread_not_its_drill() {
    let t = Target {
        thread: Some(8.0),
        ..hole(6.8, 12.0)
    };
    let got = dropped("ISO 4762", &t).unwrap_or_default();
    assert!(got.starts_with("M8 "), "{got}");
}

#[test]
fn a_blind_hole_takes_the_longest_screw_that_stays_inside() {
    let t = Target {
        thread: Some(8.0),
        blind: true,
        ..hole(6.8, 22.0)
    };
    assert_eq!(dropped("ISO 4762", &t).as_deref(), Some("M8 x 20"));
}

#[test]
fn a_hole_too_small_or_too_wide_takes_no_size() {
    assert_eq!(dropped("ISO 4017", &hole(1.0, 10.0)), None);
    // M16 is the widest near 80? No screw is that thin compared with the hole.
    assert_eq!(dropped("ISO 4762", &hole(300.0, 10.0)), None);
}

#[test]
fn a_screw_does_not_go_onto_a_shaft() {
    assert_eq!(dropped("ISO 4017", &shaft(8.0)), None);
}

#[test]
fn nuts_and_washers_go_by_the_shank_or_the_nearest_to_a_rim() {
    assert_eq!(dropped("ISO 4032", &shaft(8.0)).as_deref(), Some("M8"));
    assert_eq!(dropped("ISO 7089", &shaft(8.0)).as_deref(), Some("8"));
    // A hole's rim has no thread data: the nearest bore.
    assert_eq!(dropped("ISO 7089", &hole(9.0, 10.0)).as_deref(), Some("8"));
    assert_eq!(dropped("ISO 4032", &shaft(500.0)), None);
}

#[test]
fn the_standard_value_comes_before_the_second_source() {
    assert_eq!(dropped("DIN 1587", &shaft(10.0)).as_deref(), Some("M10"));
    assert_eq!(dropped("ISO 4032", &shaft(6.0)).as_deref(), Some("M6"));
}

#[test]
fn a_bearing_sits_on_a_shaft_by_its_bore_and_in_a_housing_by_its_outside() {
    assert_eq!(dropped("Series 62", &shaft(20.0)).as_deref(), Some("6204"));
    assert_eq!(
        dropped("Series 62", &hole(47.0, 14.0)).as_deref(),
        Some("6204")
    );
    assert_eq!(dropped("Series 62", &shaft(21.0)), None);
}

#[test]
fn every_family_that_can_be_dropped_finds_its_own_size() {
    let all = samples::all();
    for s in &all {
        let fit = Fit::of(&s.kind);
        let t = match fit {
            Fit::Into { d, length, .. } => hole(d, length.unwrap_or(1.0)),
            Fit::Over { bore } => shaft(bore),
            Fit::Bearing { bore, .. } => shaft(bore),
            Fit::Butt { diameter } => shaft(diameter),
            Fit::Manual => continue,
        };
        let fits: Vec<Fit> = all
            .iter()
            .filter(|m| m.family == s.family)
            .map(|m| Fit::of(&m.kind))
            .collect();
        assert!(
            choose(&fits, &t).is_some(),
            "{} {} finds no size of its own family",
            s.family,
            s.size
        );
    }
}

#[test]
fn the_seat_lands_on_the_circle_with_its_head_outward() {
    let c = Point3::new(10.0, 20.0, 30.0);
    let t = seat_transform(c, Vector3::y(), 0.0, false);
    assert!((t * Point3::origin() - c).norm() < 1e-9);
    assert!((t * Vector3::z() - Vector3::y()).norm() < 1e-9);
    // A rigid motion.
    assert!((t * Vector3::x()).dot(&(t * Vector3::y())).abs() < 1e-9);
    assert!(((t * Vector3::x()).cross(&(t * Vector3::y())) - t * Vector3::z()).norm() < 1e-9);
    // Flipped: the head goes in.
    let f = seat_transform(c, Vector3::y(), 0.0, true);
    assert!((f * Vector3::z() + Vector3::y()).norm() < 1e-9);
    // A countersunk head sinks along the normal, whichever way it is turned.
    let s = seat_transform(c, Vector3::y(), 3.0, false);
    assert!((s * Point3::origin() - (c - Vector3::y() * 3.0)).norm() < 1e-9);
}

#[test]
fn a_family_can_be_found_from_its_member_file() {
    let root = std::env::temp_dir().join(format!("cc-drop-{}", std::process::id()));
    let c = Catalog::load(&root);
    let f = c
        .families
        .iter()
        .position(|x| x.name == "ISO 4032")
        .unwrap_or_else(|| panic!("no family"));
    let path = c.member_file(f, 2).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(c.locate(&path), Some((f, 2)));
    assert_eq!(c.locate(&root.join("nothing.ccpart")), None);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn library_parts_carry_their_seat() {
    let s = family("ISO 4017").remove(0);
    let part = s
        .part_at(samples::Detail::Standard)
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(part.connections.len(), 1);
    assert_eq!(part.connections[0].role, ConnectionRole::Into);
    assert!(part.connections[0].edge.is_none());
}

#[test]
fn connection_names_do_not_repeat_and_a_file_keeps_them() {
    let mut list = vec![Connection::seat(ConnectionRole::Over, 8.0)];
    assert_eq!(next_name(&list), "Connection1");
    list[0].name = "Connection1".into();
    assert_eq!(next_name(&list), "Connection2");
    let text = serde_json::to_string(&list[0]).unwrap_or_default();
    let back: Connection = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(back, list[0]);
}

/// A plate with two M8 clearance holes, and the two relationships Smart Drop
/// makes: they put a bolt in the first hole, and hold after a size change.
#[test]
fn the_relationships_seat_the_part_and_survive_a_change_of_size() {
    use cadcraft3d::assembly::relation::{OriginItem, resolve_in};
    use cadcraft3d::assembly::{Constraint, ConstraintKind, GeomRef, solve};
    use cadcraft3d::components::fit::seat_transform;
    use cadcraft3d::document::{AssemblyDocument, DocRef, DocumentId};
    use cadcraft3d::kernel::Transform3;
    use cadcraft3d::part::PartDocument;
    use cadcraft3d::part::blend::EdgeRef;
    use cadcraft3d::part::hole::{HoleCenter, HoleFeature, HoleSpec, Termination};

    const TOP: &str = "Box1:Top";
    let mut plate =
        PartDocument::with_box("Box1", [100.0, 60.0, 12.0]).unwrap_or_else(|e| panic!("{e}"));
    let z = plate
        .body
        .faces()
        .find(|(_, f)| f.name.as_deref() == Some(TOP))
        .and_then(|(_, f)| f.mesh.as_ref().map(|m| m.positions[0].z))
        .unwrap_or_else(|| panic!("no top"));
    let c = Point3::new(20.0, 10.0, z);
    let (i, ids) = plate
        .create_hole_sketch(TOP, &[c])
        .unwrap_or_else(|e| panic!("{e}"));
    let mut spec = HoleSpec::simple(9.0, 12.0);
    spec.termination = Termination::ThroughAll;
    let mut def = HoleFeature::new(spec);
    def.sketch = plate.sketches[i].id.clone();
    def.centers = ids.into_iter().map(HoleCenter::Point).collect();
    plate.add_hole(def).unwrap_or_else(|e| panic!("{e}"));

    let mut a = AssemblyDocument::default();
    let doc = |n: u128| DocRef::local(DocumentId(uuid::Uuid::from_u128(n)));
    let base = a.place(doc(1), "Plate", Transform3::identity(), true);
    // Far from the hole: the solver must bring it there.
    let bolt = a.place(
        doc(2),
        "Bolt",
        Transform3::translation(200.0, -40.0, 90.0),
        false,
    );
    let rim = GeomRef::Edge {
        occurrence: base,
        sub: Vec::new(),
        edge: EdgeRef {
            faces: [TOP.to_owned(), "Hole1:Bore:1".to_owned()],
            hint: [c.x + 4.5, c.y, c.z],
        },
    };
    let origin = |item| GeomRef::Origin {
        occurrence: bolt,
        sub: Vec::new(),
        item,
    };
    a.add_constraint(Constraint::new(
        ConstraintKind::Mate,
        rim,
        origin(OriginItem::ZAxis),
    ));
    a.add_constraint(Constraint::new(
        ConstraintKind::Flush,
        GeomRef::Face {
            occurrence: base,
            sub: Vec::new(),
            name: TOP.to_owned(),
            hint: [c.x + 10.0, c.y, c.z],
        },
        origin(OriginItem::XyPlane),
    ));
    let local = |r: &GeomRef| resolve_in(&plate, r);
    let s = solve(&a, &local, None);
    assert!(s.sick.is_empty(), "{:?}", s.sick);
    assert!(s.converged, "residual {}", s.residual);
    let (_, placed) = s
        .transforms
        .iter()
        .find(|(id, _)| *id == bolt)
        .unwrap_or_else(|| panic!("the bolt did not move"));
    let at = placed * Point3::origin();
    assert!((at - c).norm() < 1e-5, "landed at {at}, hole at {c}");
    assert!((placed * Vector3::z() - Vector3::z()).norm() < 1e-5);

    // Started at its place, the solve leaves it there: no jump on commit.
    if let Some(o) = a.find_mut(bolt) {
        o.transform = seat_transform(c, Vector3::z(), 0.0, false);
    }
    let again = solve(&a, &local, None);
    assert!(again.converged);
    for (id, t) in &again.transforms {
        if *id == bolt {
            assert!((t * Point3::origin() - c).norm() < 1e-6);
        }
    }

    // Another size: the same references, only the component changes.
    a.replace(&[bolt], doc(3), "Bolt")
        .unwrap_or_else(|e| panic!("{e}"));
    let after = solve(&a, &local, None);
    assert!(after.sick.is_empty() && after.converged);
}

#[test]
fn a_bearing_and_a_ring_on_a_shaft_go_along_it_a_bolt_stays_on_the_face() {
    use cadcraft3d::components::fit::goes_in;
    let bearing = Fit::Bearing {
        bore: 20.0,
        outside: 47.0,
    };
    let bolt = Fit::Into {
        d: 8.0,
        length: Some(16.0),
        sink: 0.0,
    };
    let ring = Fit::Over { bore: 20.0 };
    // A rim: the bearing enters its housing or slides onto its shaft.
    assert!(goes_in(bearing, &hole(47.0, 12.0), true));
    assert!(goes_in(bearing, &shaft(20.0), true));
    assert!(goes_in(ring, &shaft(20.0), true));
    // A ring on a hole's rim and a bolt in a hole stay on the face.
    assert!(!goes_in(ring, &hole(9.0, 12.0), true));
    assert!(!goes_in(bolt, &hole(9.0, 12.0), true));
    // A plane picked for the seat: everything sits on it.
    assert!(!goes_in(bearing, &shaft(20.0), false));
    assert!(!goes_in(ring, &shaft(20.0), false));
}

#[test]
fn a_connection_lands_its_circle_on_the_rim_face_against_face() {
    use cadcraft3d::components::fit::landing;
    let (center, normal) = (Point3::new(0.0, 0.0, 80.0), Vector3::z());
    let (target, up) = (Point3::new(60.0, 45.0, 12.0), Vector3::z());
    let t = landing(center, normal, target, up, false);
    assert!((t * center - target).norm() < 1e-9);
    assert!((t * normal + up).norm() < 1e-9, "opposed");
    // Side by side when aligned.
    let a = landing(center, normal, target, up, true);
    assert!((a * center - target).norm() < 1e-9);
    assert!((a * normal - up).norm() < 1e-9);
}

#[test]
fn a_bolted_connection_takes_washers_a_nut_and_a_bolt_long_enough() {
    let root = std::env::temp_dir().join(format!("cc-stack-{}", std::process::id()));
    let c = Catalog::load(&root);
    let f = c
        .families
        .iter()
        .position(|x| x.name == "ISO 4017")
        .unwrap_or_else(|| panic!("no family"));
    let stack = c.bolted_stack(f, 8.0).unwrap_or_else(|| panic!("no stack"));
    assert_eq!(c.families[stack.washer.0].name, "ISO 7089");
    assert_eq!(c.families[stack.nut.0].members[stack.nut.1].size, "M8");
    assert!((stack.washer_h - 1.6).abs() < 1e-9);
    assert!((stack.nut_m - 6.8).abs() < 1e-9);
    let mut t = hole(9.0, 12.0);
    t.depth = stack.length_for(12.0, 8.0);
    let k = choose(&c.fits(f), &t).unwrap_or_else(|| panic!("no bolt"));
    assert_eq!(c.families[f].members[k].size, "M8 x 25");
    // An inch bolt takes inch parts.
    let inch = c
        .families
        .iter()
        .position(|x| x.name == "ASME B18.2.1")
        .unwrap_or_else(|| panic!("no family"));
    let s = c
        .bolted_stack(inch, 12.7)
        .unwrap_or_else(|| panic!("no stack"));
    assert_eq!(c.families[s.nut.0].name, "ASME B18.2.2");
}

/// A Hole feature's holes as a pattern's elements: moved from the first, and a
/// part not built yet is an error, so no element is lost by it.
#[test]
fn a_component_pattern_can_follow_a_hole_feature() {
    use cadcraft3d::assembly::pattern::feature_instances;
    use cadcraft3d::part::PartDocument;
    use cadcraft3d::part::hole::{HoleCenter, HoleFeature, HoleSpec, Termination};

    const TOP: &str = "Box1:Top";
    let mut plate =
        PartDocument::with_box("Box1", [100.0, 60.0, 12.0]).unwrap_or_else(|e| panic!("{e}"));
    let z = plate
        .body
        .faces()
        .find(|(_, f)| f.name.as_deref() == Some(TOP))
        .and_then(|(_, f)| f.mesh.as_ref().map(|m| m.positions[0].z))
        .unwrap_or_else(|| panic!("no top"));
    let at = [(20.0, 10.0), (50.0, 10.0), (20.0, 40.0)].map(|(x, y)| Point3::new(x, y, z));
    let (i, ids) = plate
        .create_hole_sketch(TOP, &at)
        .unwrap_or_else(|e| panic!("{e}"));
    let mut spec = HoleSpec::simple(9.0, 12.0);
    spec.termination = Termination::ThroughAll;
    let mut def = HoleFeature::new(spec);
    def.sketch = plate.sketches[i].id.clone();
    def.centers = ids.into_iter().map(HoleCenter::Point).collect();
    let id = plate.add_hole(def).unwrap_or_else(|e| panic!("{e}"));

    let moves = feature_instances(&plate, &id).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(moves.len(), 2);
    assert_eq!(moves[0].0, 2);
    let mut offsets: Vec<Vector3> = moves
        .iter()
        .map(|m| m.1 * Point3::origin() - Point3::origin())
        .collect();
    offsets.sort_by(|a, b| a.x.total_cmp(&b.x));
    assert!((offsets[0] - Vector3::new(0.0, 30.0, 0.0)).norm() < 1e-6);
    assert!((offsets[1] - Vector3::new(30.0, 0.0, 0.0)).norm() < 1e-6);

    // As a file opens it: the holes are not listed yet. An error, not "no holes".
    let text = serde_json::to_string(&plate).unwrap_or_default();
    let loaded: PartDocument = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}"));
    assert!(feature_instances(&loaded, &id).is_err());
}

/// The pattern from Follow Pattern: elements appear when the Hole feature gains
/// a hole and go when it loses one.
#[test]
fn the_copies_follow_the_holes_of_the_feature() {
    use cadcraft3d::assembly::GeomRef;
    use cadcraft3d::assembly::pattern::{
        ComponentPattern, Layout, Lookup, feature_instances, place,
    };
    use cadcraft3d::assembly::relation::resolve_in;
    use cadcraft3d::document::{AssemblyDocument, DocRef, DocumentId};
    use cadcraft3d::kernel::Transform3;
    use cadcraft3d::part::PartDocument;
    use cadcraft3d::part::hole::{HoleCenter, HoleFeature, HoleSpec, Termination};

    const TOP: &str = "Box1:Top";
    let plate = |at: &[(f64, f64)]| {
        let mut p =
            PartDocument::with_box("Box1", [100.0, 60.0, 12.0]).unwrap_or_else(|e| panic!("{e}"));
        let z = p
            .body
            .faces()
            .find(|(_, f)| f.name.as_deref() == Some(TOP))
            .and_then(|(_, f)| f.mesh.as_ref().map(|m| m.positions[0].z))
            .unwrap_or_else(|| panic!("no top"));
        let pts: Vec<Point3> = at.iter().map(|&(x, y)| Point3::new(x, y, z)).collect();
        let (i, ids) = p
            .create_hole_sketch(TOP, &pts)
            .unwrap_or_else(|e| panic!("{e}"));
        let mut spec = HoleSpec::simple(9.0, 12.0);
        spec.termination = Termination::ThroughAll;
        let mut def = HoleFeature::new(spec);
        def.sketch = p.sketches[i].id.clone();
        def.centers = ids.into_iter().map(HoleCenter::Point).collect();
        p.add_hole(def).unwrap_or_else(|e| panic!("{e}"));
        p
    };
    let two = plate(&[(20.0, 10.0), (50.0, 10.0)]);
    let three = plate(&[(20.0, 10.0), (50.0, 10.0), (80.0, 10.0)]);
    let doc = |n: u128| DocRef::local(DocumentId(uuid::Uuid::from_u128(n)));
    let mut a = AssemblyDocument::default();
    let base = a.place(doc(1), "Plate", Transform3::identity(), true);
    let bolt = a.place(
        doc(2),
        "Bolt",
        Transform3::translation(20.0, 10.0, 12.0),
        false,
    );
    a.patterns.push(ComponentPattern {
        id: 1,
        name: "Component Pattern 1".into(),
        sources: vec![bolt],
        layout: Layout::Feature {
            occurrence: base,
            feature: "Hole1".into(),
        },
        elements: Vec::new(),
    });
    let run = |a: &mut AssemblyDocument, part: &PartDocument| {
        let local = |r: &GeomRef| resolve_in(part, r);
        let feature = |_: u32, id: &str| feature_instances(part, id);
        place(
            a,
            &Lookup {
                local: &local,
                feature: &feature,
            },
        )
    };
    run(&mut a, &two).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(a.occurrences.len(), 3);
    let at = |a: &AssemblyDocument, k: usize| a.occurrences[k].transform * Point3::origin();
    assert!((at(&a, 2) - Point3::new(50.0, 10.0, 12.0)).norm() < 1e-6);

    run(&mut a, &three).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(a.occurrences.len(), 4, "a new hole adds a copy");
    assert!((at(&a, 3) - Point3::new(80.0, 10.0, 12.0)).norm() < 1e-6);

    run(&mut a, &two).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(a.occurrences.len(), 3, "a removed hole takes its copy");
}

/// A flange finds its size by the pipe: a weld neck flange butts on the pipe's end, a
/// slip-on goes over it, a blind flange meets a flange's rim. None goes into a hole.
#[test]
fn a_flange_lands_on_its_pipe() {
    let dropped = |family: &str, t: &Target| {
        let all = samples::all();
        let members: Vec<_> = all.iter().filter(|s| s.family == family).collect();
        let fits: Vec<Fit> = members.iter().map(|s| Fit::of(&s.kind)).collect();
        choose(&fits, t).map(|i| members[i].size.clone())
    };
    assert_eq!(
        dropped("EN 1092-1 Type 11", &shaft(114.3)).as_deref(),
        Some("DN100")
    );
    assert_eq!(
        dropped("EN 1092-1 Type 01", &shaft(114.3)).as_deref(),
        Some("DN100")
    );
    assert_eq!(
        dropped("EN 1092-1 Type 05", &shaft(220.0)).as_deref(),
        Some("DN100")
    );
    assert_eq!(dropped("EN 1092-1 Type 11", &hole(114.3, 10.0)), None);
    assert_eq!(dropped("EN 1092-1 Type 11", &shaft(100.0)), None);
    // It continues the pipe; a slip-on runs back along it.
    let t = shaft(114.3);
    assert!(!goes_in(Fit::Butt { diameter: 114.3 }, &t, true));
    assert!(goes_in(Fit::Over { bore: 116.0 }, &t, true));
}
