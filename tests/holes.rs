//! Hole and Thread (`docs/spec/2.3-hole.md`, `docs/spec/2.9-thread.md`):
//! every hole type and seat on a 40 × 30 × 20 block, with analytic
//! volumes; tapped holes cosmetic and modeled; Thread on a shaft.
//! Cross-feature cases (fillet, chamfer, shell, edit) are in
//! `tests/matrix.rs`.

use std::f64::consts::PI;

use cadcraft3d::kernel::{Point3, Real, Surface};
use cadcraft3d::part::hole::HoleCenter;
use cadcraft3d::part::standards::{self, Standards};
use cadcraft3d::part::{
    DrillPoint, ExtrudeFeature, FastenerFit, Feature, HoleFeature, HoleSpec, OriginPlane,
    PartDocument, ProfileRef, SeatType, SketchOptions, SketchPlacement, Termination, ThreadFeature,
    ThreadSpec,
};
use cadcraft3d::sketch::Point2;

const TOP: &str = "Extrusion1:End";
const BLOCK: Real = 40.0 * 30.0 * 20.0;

fn volume(part: &PartDocument) -> Real {
    let opts = cadcraft3d::kernel::TessellationOptions::default();
    part.body
        .solids()
        .map(|(sid, _)| {
            let m = part.body.tessellate_solid(sid, &opts).unwrap();
            m.faces
                .triangles
                .iter()
                .map(|t| {
                    let [p, q, s] = t.map(|i| m.faces.positions[i as usize].coords);
                    p.dot(&q.cross(&s)) / 6.0
                })
                .sum::<Real>()
        })
        .sum()
}

/// A rectangle W×D extruded H on XY.
fn plate(w: Real, d: Real, h: Real) -> PartDocument {
    let mut part = PartDocument::default();
    let opts = SketchOptions {
        auto_project_origin: false,
        ..Default::default()
    };
    let i = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts)
        .unwrap();
    let sk = &mut part.sketches[i].sketch;
    sk.add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(w, d))
        .unwrap();
    let r = sk.regions();
    part.add_extrude(
        i,
        ExtrudeFeature::new(vec![ProfileRef::of(&r[0], r[0].interior_point())], h),
    )
    .unwrap();
    part
}

fn block() -> PartDocument {
    plate(40.0, 30.0, 20.0)
}

/// Holes `spec` at `centers` (x, y) on the block's top face (z 20).
fn holes_at(
    part: &mut PartDocument,
    spec: HoleSpec,
    centers: &[(Real, Real)],
) -> Result<String, String> {
    let z = part
        .body
        .faces()
        .find(|(_, f)| f.name.as_deref() == Some(TOP))
        .and_then(|(_, f)| f.mesh.as_ref().map(|m| m.positions[0].z))
        .unwrap_or(20.0);
    let pts: Vec<Point3> = centers.iter().map(|&(x, y)| Point3::new(x, y, z)).collect();
    let (i, ids) = part
        .create_hole_sketch(TOP, &pts)
        .map_err(|e| e.to_string())?;
    let mut def = HoleFeature::new(spec);
    def.sketch = part.sketches[i].id.clone();
    def.centers = ids.into_iter().map(HoleCenter::Point).collect();
    part.add_hole(def).map_err(|e| e.to_string())
}

fn check(part: &PartDocument, want: Real, tol: Real) {
    assert!(part.exact, "exact kernel");
    assert!(part.feature_health.is_empty(), "{:?}", part.feature_health);
    for (sid, _) in part.body.solids() {
        part.body.validate_solid(sid).unwrap();
    }
    let v = volume(part);
    assert!((v - want).abs() <= tol * want, "volume {v:.3} vs {want:.3}");
}

fn cone(r: Real, angle: Real) -> Real {
    let h = r / (angle / 2.0).tan();
    PI * r * r * h / 3.0
}

// ------------------------------------------------------------- simple ---

#[test]
fn simple_blind_flat() {
    let mut part = block();
    let mut s = HoleSpec::simple(6.0, 10.0);
    s.drill_point = DrillPoint::Flat;
    holes_at(&mut part, s, &[(20.0, 15.0)]).unwrap();
    check(&part, BLOCK - PI * 9.0 * 10.0, 2e-4);
    let names: Vec<String> = part
        .body
        .faces()
        .filter_map(|(_, f)| f.name.clone())
        .collect();
    assert!(names.iter().any(|n| n == "Hole1:Bore:1"), "{names:?}");
    assert!(names.iter().any(|n| n == "Hole1:Bottom:1"), "{names:?}");
    // The bore is an exact cylinder.
    let bore = part
        .body
        .faces()
        .find(|(_, f)| f.name.as_deref() == Some("Hole1:Bore:1"))
        .unwrap()
        .1;
    assert!(
        matches!(part.body.surface(bore.surface), Some(Surface::Cylinder(c)) if (c.radius - 3.0).abs() < 1e-9)
    );
}

#[test]
fn simple_blind_118_degree_point() {
    let mut part = block();
    holes_at(&mut part, HoleSpec::simple(6.0, 10.0), &[(20.0, 15.0)]).unwrap();
    let want = BLOCK - PI * 9.0 * 10.0 - cone(3.0, 118f64.to_radians());
    check(&part, want, 2e-4);
    assert!(
        part.body
            .faces()
            .any(|(_, f)| f.name.as_deref() == Some("Hole1:Tip:1"))
    );
}

#[test]
fn simple_through_all() {
    let mut part = block();
    let mut s = HoleSpec::simple(6.0, 10.0);
    s.termination = Termination::ThroughAll;
    holes_at(&mut part, s, &[(10.0, 10.0), (30.0, 20.0)]).unwrap();
    check(&part, BLOCK - 2.0 * PI * 9.0 * 20.0, 2e-4);
}

#[test]
fn simple_to_the_bottom_face() {
    let mut part = block();
    let bottom = part
        .body
        .faces()
        .find(|(_, f)| f.name.as_deref() == Some("Extrusion1:Start"))
        .and_then(|(fid, _)| {
            cadcraft3d::part::shell::face_ref(&part.body, fid, Some(Point3::new(20.0, 15.0, 0.0)))
        })
        .unwrap();
    let mut s = HoleSpec::simple(6.0, 10.0);
    s.termination = Termination::To(bottom);
    holes_at(&mut part, s, &[(20.0, 15.0)]).unwrap();
    check(&part, BLOCK - PI * 9.0 * 20.0, 2e-4);
}

#[test]
fn simple_flipped_misses_the_body() {
    let mut part = block();
    let mut s = HoleSpec::simple(6.0, 10.0);
    s.flip = true;
    let e = holes_at(&mut part, s, &[(20.0, 15.0)]).unwrap_err();
    assert!(e.contains("does not intersect"), "{e}");
    assert_eq!(part.features.len(), 1, "failure leaves the part unchanged");
}

#[test]
fn bad_values_are_refused() {
    let mut part = block();
    let e = holes_at(&mut part, HoleSpec::simple(0.0, 10.0), &[(20.0, 15.0)]).unwrap_err();
    assert!(e.contains("diameter"), "{e}");
    let e = holes_at(&mut part, HoleSpec::simple(6.0, -1.0), &[(20.0, 15.0)]).unwrap_err();
    assert!(e.contains("depth"), "{e}");
    let mut def = HoleFeature::new(HoleSpec::simple(6.0, 10.0));
    def.centers.clear();
    let e = part.add_hole(def).unwrap_err().to_string();
    assert!(e.contains("centres"), "{e}");
    assert_eq!(part.features.len(), 1);
}

// -------------------------------------------------------------- seats ---

#[test]
fn counterbore_iso_4762_m6() {
    let t = Standards::builtin();
    let mut s = HoleSpec::clearance(&t, "ISO 273", "M6", FastenerFit::Normal).unwrap();
    assert!(s.counterbore_from(&t, "Socket Head Cap Screw ISO 4762", "M6"));
    let mut part = block();
    holes_at(&mut part, s, &[(20.0, 15.0)]).unwrap();
    let want = BLOCK - PI * (5.5 * 5.5 * 6.4 + 3.3 * 3.3 * 13.6);
    check(&part, want, 2e-4);
    for n in ["Hole1:CBore:1", "Hole1:CBoreFloor:1", "Hole1:Bore:1"] {
        assert!(
            part.body.faces().any(|(_, f)| f.name.as_deref() == Some(n)),
            "{n}"
        );
    }
}

#[test]
fn countersink_90() {
    let mut s = HoleSpec::simple(6.6, 10.0);
    s.termination = Termination::ThroughAll;
    s.seat = SeatType::Countersink;
    s.csink_diameter = 12.0;
    s.csink_angle = 90f64.to_radians();
    let mut part = block();
    holes_at(&mut part, s, &[(20.0, 15.0)]).unwrap();
    let h = 6.0 - 3.3; // the cone meets the bore 2.7 deep (45°)
    let frustum = PI * h / 3.0 * (36.0 + 6.0 * 3.3 + 3.3 * 3.3);
    check(&part, BLOCK - frustum - PI * 3.3 * 3.3 * (20.0 - h), 2e-4);
    assert!(
        part.body
            .faces()
            .any(|(_, f)| f.name.as_deref() == Some("Hole1:CSink:1"))
    );
}

#[test]
fn countersink_82_ansi() {
    let t = Standards::builtin();
    let mut s = HoleSpec::clearance(&t, "ASME B18.2.8", "1/4", FastenerFit::Normal).unwrap();
    assert!(s.countersink_from(&t, "Flat Countersunk Socket Cap Screw ASME B18.3", "1/4"));
    assert!((s.csink_angle.to_degrees() - 82.0).abs() < 1e-9);
    let r = s.diameter / 2.0;
    let rc = s.csink_diameter / 2.0;
    let mut part = block();
    holes_at(&mut part, s, &[(20.0, 15.0)]).unwrap();
    let h = (rc - r) / 41f64.to_radians().tan();
    let frustum = PI * h / 3.0 * (rc * rc + rc * r + r * r);
    check(&part, BLOCK - frustum - PI * r * r * (20.0 - h), 3e-4);
}

#[test]
fn spotface_depth_from_its_floor() {
    let mut s = HoleSpec::simple(9.0, 10.0);
    s.drill_point = DrillPoint::Flat;
    s.seat = SeatType::Spotface;
    s.seat_diameter = 20.0;
    s.seat_depth = 1.0;
    let mut part = block();
    holes_at(&mut part, s, &[(20.0, 15.0)]).unwrap();
    // Spotface 1 deep, then the hole 10 deep from its floor.
    check(&part, BLOCK - PI * (100.0 * 1.0 + 20.25 * 10.0), 2e-4);
    assert!(
        part.body
            .faces()
            .any(|(_, f)| f.name.as_deref() == Some("Hole1:SpotfaceFloor:1"))
    );
}

#[test]
fn clearance_m6_from_the_iso_table() {
    let t = Standards::builtin();
    for (fit, d) in [
        (FastenerFit::Close, 6.4),
        (FastenerFit::Normal, 6.6),
        (FastenerFit::Loose, 7.0),
    ] {
        let s = HoleSpec::clearance(&t, "ISO 273", "M6", fit).unwrap();
        assert_eq!(s.diameter, d);
        let mut part = block();
        holes_at(&mut part, s, &[(20.0, 15.0)]).unwrap();
        check(&part, BLOCK - PI * d * d / 4.0 * 20.0, 2e-4);
        assert_eq!(part.holes()[0].spec.note(), format!("Ø{d} THRU"));
    }
}

// ------------------------------------------------------------ threads ---

fn m8() -> ThreadSpec {
    ThreadSpec::from_table(&Standards::builtin(), standards::ISO_METRIC, "M8", true).unwrap()
}

/// The volume a modeled internal groove removes per turn beyond the bore
/// (radius `rb`): 2π ∫ r·w(r) dr of the section.
fn groove_per_turn(t: &ThreadSpec, rb: Real) -> Real {
    let g = t.internal_groove(rb).unwrap();
    let (r0, w0, r1, w1) = (g[0][0], -g[0][1], g[1][0], -g[1][1]);
    let n = 4000;
    let mut s = 0.0;
    for i in 0..n {
        let r = rb + (r1 - rb) * (i as Real + 0.5) / n as Real;
        let w = w1 + (w0 - w1) * (r1 - r) / (r1 - r0);
        s += r * 2.0 * w * (r1 - rb) / n as Real;
    }
    2.0 * PI * s
}

#[test]
fn tapped_m8_cosmetic() {
    let mut part = block();
    let s = HoleSpec::tapped(m8(), 15.0);
    holes_at(&mut part, s, &[(20.0, 15.0)]).unwrap();
    let r = m8().minor / 2.0;
    check(
        &part,
        BLOCK - PI * r * r * 15.0 - cone(r, 118f64.to_radians()),
        2e-4,
    );
    // Cosmetic: a display hint at the major diameter, the data in the note.
    assert_eq!(part.cosmetic_threads.len(), 1);
    assert!((part.cosmetic_threads[0].radius - 4.0).abs() < 1e-12);
    assert!((part.cosmetic_threads[0].length - 15.0).abs() < 1e-9);
    assert_eq!(
        part.holes()[0].spec.note_line(),
        "M8x1.25 - 6H ↧ 15 / Ø6.75 ↧ 15"
    );
}

#[test]
fn tapped_m8_modeled_through() {
    let mut part = block();
    let mut t = m8();
    t.modeled = true;
    let mut s = HoleSpec::tapped(t.clone(), 15.0);
    s.termination = Termination::ThroughAll;
    holes_at(&mut part, s, &[(20.0, 15.0)]).unwrap();
    let rb = t.minor / 2.0;
    let bored = BLOCK - PI * rb * rb * 20.0;
    let groove = groove_per_turn(&t, rb) * 20.0 / t.pitch;
    let v = volume(&part);
    // Between the minor and the major cylinder…
    assert!(v < bored && v > bored - PI * (16.0 - rb * rb) * 20.0, "{v}");
    // … and the groove's analytic volume (36 stations a turn: −1 %).
    assert!(
        ((bored - v) - groove).abs() < 0.03 * groove,
        "{} vs {groove}",
        bored - v
    );
    check(&part, bored - groove, 1e-3);
    assert!(part.cosmetic_threads.is_empty());
    // No faceting of the rest: the block's sides stay exact planes, and the
    // bore below… (through: all of it is threaded) the top face keeps its name.
    let thread_faces = part
        .body
        .faces()
        .filter(|(_, f)| f.name.as_deref() == Some("Hole1:Thread:1"))
        .count();
    assert!(thread_faces > 100, "{thread_faces} groove facets");
    let sides = part
        .body
        .faces()
        .filter(|(_, f)| {
            f.name
                .as_deref()
                .is_some_and(|n| n.starts_with("Extrusion1:Side"))
        })
        .count();
    assert_eq!(sides, 4, "the outside is untouched");
}

#[test]
fn tapped_m8_modeled_blind_keeps_the_bottom_exact() {
    let mut part = block();
    let mut t = m8();
    t.modeled = true;
    t.full_depth = false;
    t.depth = 10.0;
    let s = HoleSpec::tapped(t.clone(), 15.0);
    holes_at(&mut part, s, &[(20.0, 15.0)]).unwrap();
    let rb = t.minor / 2.0;
    let bored = BLOCK - PI * rb * rb * 15.0 - cone(rb, 118f64.to_radians());
    // The groove starts with its lowest point at the thread depth: its
    // centre climbs from 10 − w0 below the face (w0: half its width there).
    let w0 = -t.internal_groove(rb).unwrap()[0][1];
    let groove = groove_per_turn(&t, rb) * (10.0 - w0) / t.pitch;
    let v = volume(&part);
    // 36 stations a turn (−1 %) and the threaded zone's bore faceted to
    // 0.01 mm (an inscribed polygon: +1.4 mm³ of wall here, −2 %).
    assert!(
        ((bored - v) - groove).abs() < 0.06 * groove,
        "{} vs {groove}",
        bored - v
    );
    // Below the thread the bore stays an exact cylinder and the drill point
    // a few faces (not facets).
    let exact_bore = part.body.faces().any(|(_, f)| {
        f.name.as_deref() == Some("Hole1:Bore:1")
            && matches!(part.body.surface(f.surface), Some(Surface::Cylinder(_)))
    });
    assert!(exact_bore, "the bore below the thread stays exact");
    let tips = part
        .body
        .faces()
        .filter(|(_, f)| f.name.as_deref() == Some("Hole1:Tip:1"))
        .count();
    assert!((1..5).contains(&tips), "{tips} drill point faces");
}

/// A modeled M3 through a plate `w` wide and `h` thick.
fn m3_through(w: Real, h: Real) -> (PartDocument, Result<String, String>) {
    let mut part = plate(w, w, h);
    let mut t =
        ThreadSpec::from_table(&Standards::builtin(), standards::ISO_METRIC, "M3", true).unwrap();
    t.modeled = true;
    let mut s = HoleSpec::tapped(t, h);
    s.termination = Termination::ThroughAll;
    let r = holes_at(&mut part, s, &[(20.0, 20.0)]);
    (part, r)
}

fn groove_faces(part: &PartDocument) -> usize {
    part.body
        .faces()
        .filter(|(_, f)| f.name.as_deref() == Some("Hole1:Thread:1"))
        .count()
}

#[test]
fn modeled_thread_in_a_thin_plate_of_a_large_part() {
    // The owner's "the thread makes no geometry": on a 150 mm plate 2 mm
    // thick the tool starts 2.1 mm above the face, the middle of the thread
    // zone lay above the plate and the groove was skipped without a word.
    let (part, r) = m3_through(150.0, 2.0);
    r.unwrap();
    assert!(part.feature_health.is_empty(), "{:?}", part.feature_health);
    assert!(
        groove_faces(&part) > 50,
        "{} groove faces",
        groove_faces(&part)
    );
}

#[test]
fn m3_in_a_300_mm_plate_is_threaded() {
    // 300 mm wide, 3 thick, M3: the zone's material cut out of so large a
    // body came out imprecise (planes tilted 1.7e-6 rad, circles 1 µm off)
    // and the faceted cut failed (refused with "use a cosmetic thread").
    // Now such slabs are cut out of the body near the zone
    // (`kernel::exact::slab_threads`); more cases in `tests/hole_threads.rs`.
    let (part, r) = m3_through(300.0, 3.0);
    r.unwrap();
    assert!(part.feature_health.is_empty(), "{:?}", part.feature_health);
    assert!(
        groove_faces(&part) > 50,
        "{} groove faces",
        groove_faces(&part)
    );
}

#[test]
fn printing_clearance_widens_a_modeled_thread() {
    let mut t = m8();
    t.modeled = true;
    let mut a = block();
    let mut s = HoleSpec::tapped(t.clone(), 15.0);
    s.termination = Termination::ThroughAll;
    holes_at(&mut a, s.clone(), &[(20.0, 15.0)]).unwrap();
    let mut b = block();
    s.thread.as_mut().unwrap().clearance = 0.2;
    assert_eq!(s.bore_radius(), t.minor / 2.0 + 0.2);
    holes_at(&mut b, s, &[(20.0, 15.0)]).unwrap();
    let (va, vb) = (volume(&a), volume(&b));
    // The bore grows by 0.2 in radius, the groove moves out with it.
    let more = PI * ((t.minor / 2.0 + 0.2).powi(2) - (t.minor / 2.0).powi(2)) * 20.0;
    assert!(va - vb > 0.8 * more, "{va} {vb} {more}");
    // SolidCraft #2705 / #2969: the diameters stay the table's; only the
    // geometry is offset.
    let h = b.holes()[0].spec.thread.clone().unwrap();
    assert_eq!((h.major, h.minor), (t.major, t.minor));
}

#[test]
fn unc_quarter_20() {
    let t = Standards::builtin();
    let th = ThreadSpec::from_table(&t, standards::ANSI_UNIFIED, "1/4-20 UNC", true).unwrap();
    assert_eq!(th.class, "2B");
    let mut s = HoleSpec::tapped(th.clone(), 12.0);
    s.termination = Termination::ThroughAll;
    let mut part = block();
    holes_at(&mut part, s, &[(20.0, 15.0)]).unwrap();
    let r = th.minor / 2.0;
    check(&part, BLOCK - PI * r * r * 20.0, 2e-4);
    assert_eq!(part.holes()[0].spec.note(), "1/4-20 UNC - 2B THRU");
}

#[test]
fn thread_on_a_shaft_cosmetic_and_modeled() {
    // A Ø8 shaft 20 long.
    let mut part = PartDocument::default();
    let i = part
        .create_sketch(
            SketchPlacement::on_origin_plane(OriginPlane::XY),
            &SketchOptions::default(),
        )
        .unwrap();
    part.sketches[i]
        .sketch
        .add_circle(Point2::new(0.0, 0.0), 4.0)
        .unwrap();
    let r = part.sketches[i].sketch.regions();
    let disc = r.iter().find(|r| r.holes.is_empty()).unwrap();
    part.add_extrude(
        i,
        ExtrudeFeature::new(vec![ProfileRef::of(disc, disc.interior_point())], 20.0),
    )
    .unwrap();
    let side = part
        .body
        .faces()
        .find(|(_, f)| f.name.as_deref().is_some_and(|n| n.contains(":Side")))
        .map(|(fid, _)| fid)
        .unwrap();
    let face =
        cadcraft3d::part::shell::face_ref(&part.body, side, Some(Point3::new(4.0, 0.0, 19.0)))
            .unwrap();
    let ext =
        ThreadSpec::from_table(&Standards::builtin(), standards::ISO_METRIC, "M8", false).unwrap();
    assert_eq!(ext.class, "6g");
    let mut cos = part.clone();
    cos.add_thread(ThreadFeature::new(face.clone(), ext.clone()))
        .unwrap();
    let v0 = PI * 16.0 * 20.0;
    check(&cos, v0, 3e-3);
    assert_eq!(cos.cosmetic_threads.len(), 1);
    // Started at the end nearest the click (z 20), going down.
    let c = &cos.cosmetic_threads[0];
    assert!((c.start.z - 20.0).abs() < 1e-9 && c.axis.z < -0.99, "{c:?}");
    // A wrong size is refused (SolidCraft #2705 / #2969).
    let m12 =
        ThreadSpec::from_table(&Standards::builtin(), standards::ISO_METRIC, "M12", false).unwrap();
    let e = part
        .clone()
        .add_thread(ThreadFeature::new(face.clone(), m12))
        .unwrap_err();
    assert!(e.to_string().contains("does not fit"), "{e}");
    // Modeled, 10 long from the top end.
    let mut m = ext.clone();
    m.modeled = true;
    m.full_depth = false;
    m.depth = 10.0;
    part.add_thread(ThreadFeature::new(face, m)).unwrap();
    let v = volume(&part);
    assert!(part.feature_health.is_empty(), "{:?}", part.feature_health);
    let removed = v0 - v;
    // External ISO profile: the gaps between teeth from the root to the
    // crest, 10 turns' worth… roughly 8 turns of P·(5H/8)·r̄ — between
    // the bounds of the ring it lies in.
    let h = 1.25 * 3f64.sqrt() / 2.0;
    let ring = PI * (16.0 - (4.0 - 5.0 * h / 8.0).powi(2)) * 10.0;
    assert!(
        removed > 0.3 * ring && removed < 0.7 * ring,
        "{removed} of {ring}"
    );
}

#[test]
fn hole_on_a_curved_face_is_refused() {
    // The side of a cylinder is not a planar face for the hole sketch.
    let mut part = PartDocument::default();
    let i = part
        .create_sketch(
            SketchPlacement::on_origin_plane(OriginPlane::XY),
            &SketchOptions::default(),
        )
        .unwrap();
    part.sketches[i]
        .sketch
        .add_circle(Point2::new(0.0, 0.0), 10.0)
        .unwrap();
    let r = part.sketches[i].sketch.regions();
    let disc = r.iter().find(|r| r.holes.is_empty()).unwrap();
    part.add_extrude(
        i,
        ExtrudeFeature::new(vec![ProfileRef::of(disc, disc.interior_point())], 20.0),
    )
    .unwrap();
    let side = part
        .body
        .faces()
        .find_map(|(_, f)| f.name.clone().filter(|n| n.contains(":Side")))
        .unwrap();
    let e = part
        .create_hole_sketch(&side, &[Point3::new(10.0, 0.0, 10.0)])
        .unwrap_err();
    assert!(e.to_string().contains("plan"), "{e}");
}

#[test]
fn concentric_to_a_circular_edge() {
    // A Ø20 boss on the block, then a Ø8 hole concentric to its top edge.
    let mut part = block();
    let opts = SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    };
    let j = part
        .create_sketch(SketchPlacement::on_face(TOP), &opts)
        .unwrap();
    let plane = part.sketches[j].sketch.plane.clone();
    let c = plane.project(&Point3::new(20.0, 15.0, 20.0));
    part.sketches[j].sketch.add_circle(c, 10.0).unwrap();
    let r = part.sketches[j].sketch.regions();
    let disc = r.iter().find(|r| r.holes.is_empty()).unwrap();
    let mut ex = ExtrudeFeature::new(vec![ProfileRef::of(disc, disc.interior_point())], 5.0);
    ex.operation = cadcraft3d::part::Operation::Join;
    part.add_extrude(j, ex).unwrap();
    let edge = part
        .body
        .edges()
        .filter_map(|(e, _)| cadcraft3d::part::blend::edge_ref(&part.body, e))
        .find(|r| (r.hint[2] - 25.0).abs() < 1e-6)
        .unwrap();
    let mut def = HoleFeature::new(HoleSpec::simple(8.0, 10.0));
    def.spec.drill_point = DrillPoint::Flat;
    def.centers = vec![HoleCenter::Concentric(edge)];
    part.add_hole(def).unwrap();
    let boss = PI * 100.0 * 5.0;
    check(&part, BLOCK + boss - PI * 16.0 * 10.0, 2e-3);
    let h = &part.holes()[0];
    assert!(
        (h.origin - Point3::new(20.0, 15.0, 25.0)).norm() < 1e-6,
        "{h:?}"
    );
    assert!(h.direction.z < -0.99);
}

#[test]
fn holes_group_across_features() {
    let t = Standards::builtin();
    let s = HoleSpec::clearance(&t, "ISO 273", "M6", FastenerFit::Normal).unwrap();
    let mut part = block();
    holes_at(&mut part, s.clone(), &[(5.0, 5.0), (35.0, 5.0)]).unwrap();
    holes_at(&mut part, s, &[(5.0, 25.0), (35.0, 25.0)]).unwrap();
    holes_at(&mut part, HoleSpec::tapped(m8(), 12.0), &[(20.0, 15.0)]).unwrap();
    let holes = part.holes();
    assert_eq!(holes.len(), 5);
    assert_eq!(
        (holes[2].feature.as_str(), holes[2].occurrence),
        ("Hole2", 1)
    );
    assert_eq!(holes[0].start_face.as_deref(), Some(TOP));
    let g = part.hole_groups();
    assert_eq!(g.len(), 2);
    assert_eq!(g[0].label(), "4x Ø6.6 THRU");
    assert_eq!(g[1].label(), "M8x1.25 - 6H ↧ 12\nØ6.75 ↧ 12");
}

#[test]
fn edit_hole_size_and_roundtrip() {
    let mut part = block();
    holes_at(&mut part, HoleSpec::simple(6.0, 10.0), &[(20.0, 15.0)]).unwrap();
    let k = part.features.len() - 1;
    let Some(Feature::Hole(mut h)) = part.features.get(k).cloned() else {
        panic!("a hole")
    };
    h.spec.diameter = 10.0;
    h.spec.drill_point = DrillPoint::Flat;
    part.replace_feature(k, Feature::Hole(h.clone())).unwrap();
    check(&part, BLOCK - PI * 25.0 * 10.0, 2e-4);
    // A failing edit leaves the old definition.
    h.spec.diameter = -1.0;
    assert!(part.replace_feature(k, Feature::Hole(h)).is_err());
    check(&part, BLOCK - PI * 25.0 * 10.0, 2e-4);
    // Save and load: the same part.
    let json = serde_json::to_string(&part).unwrap();
    let mut back: PartDocument = serde_json::from_str(&json).unwrap();
    back.regenerate().unwrap();
    check(&back, BLOCK - PI * 25.0 * 10.0, 2e-4);
    // Parameters: the diameter is one.
    assert!(
        part.parameter_table()
            .iter()
            .any(|r| r.consumed_by == "Hole1" && (r.value - 10.0).abs() < 1e-9)
    );
}

#[test]
fn thirty_six_holes_in_one_feature() {
    let mut part = plate(100.0, 100.0, 10.0);
    let mut s = HoleSpec::simple(5.0, 10.0);
    s.termination = Termination::ThroughAll;
    let centers: Vec<(Real, Real)> = (0..36)
        .map(|i| (10.0 + 16.0 * (i % 6) as Real, 10.0 + 16.0 * (i / 6) as Real))
        .collect();
    let t0 = std::time::Instant::now();
    holes_at(&mut part, s, &centers).unwrap();
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    check(&part, 100_000.0 - 36.0 * PI * 6.25 * 10.0, 5e-4);
    assert_eq!(part.holes().len(), 36);
    assert_eq!(part.hole_groups()[0].label(), "36x Ø5 THRU");
    println!("36 holes: {ms:.0} ms");
}

/// Holes copied by a Rectangular Pattern are cut (the Hole feature's
/// tools are patterned) and are holes for the drawing too: a 2×2 pattern
/// of one M6 clearance hole gives 4 bores and one group "4x Ø6.6 THRU";
/// a Mirror of a Hole feature likewise.
#[test]
fn pattern_and_mirror_of_a_hole_feature() {
    use cadcraft3d::part::{LineRef, PatternDirection, PatternFeature, PatternKind, PullRef};
    let t = Standards::builtin();
    let s = HoleSpec::clearance(&t, "ISO 273", "M6", FastenerFit::Normal).unwrap();
    let mut part = block();
    holes_at(&mut part, s, &[(10.0, 8.0)]).unwrap();
    let dir = |axis: usize, count: u32, spacing: Real| {
        let mut d = PatternDirection::new();
        d.dir = Some(PullRef::Line(LineRef::Origin(axis)));
        d.count = count;
        d.spacing = spacing;
        d
    };
    let mut p = PatternFeature::rectangular();
    p.features = vec!["Hole1".into()];
    p.kind = PatternKind::Rectangular {
        first: dir(0, 2, 20.0),
        second: Some(dir(1, 2, 14.0)),
    };
    part.add_pattern(p).unwrap();
    let bore = PI * 3.3 * 3.3 * 20.0;
    // Display-triangle volume: ~2 mm³ low per bore.
    check(&part, BLOCK - 4.0 * bore, 5e-4);
    assert_eq!(part.holes().len(), 4, "{:?}", part.holes());
    let g = part.hole_groups();
    assert_eq!(g.len(), 1, "{g:?}");
    assert_eq!(g[0].label(), "4x Ø6.6 THRU");

    // Mirror of the Hole feature across the plane x = 20 (YZ moved): the
    // origin YZ plane mirrors x = 10 to x = −10, off the block; use a
    // block centred on the origin instead.
    let mut part = plate(40.0, 30.0, 20.0);
    holes_at(&mut part, HoleSpec::simple(6.0, 10.0), &[(30.0, 15.0)]).unwrap();
    let mut m = PatternFeature::mirror();
    m.features = vec!["Hole1".into()];
    m.kind = PatternKind::Mirror {
        plane: Some(cadcraft3d::part::PlaneRef::Origin(OriginPlane::YZ)),
    };
    // The mirror of x = 30 is x = −30: off the block, refused cleanly.
    let n = part.features.len();
    assert!(part.add_pattern(m).is_err());
    assert_eq!(part.features.len(), n);
}

/// Two blind 118° holes in one feature (the GUI test's case): a valid body
/// (a cone tip's edges were run twice the same way: a non-manifold edge).
#[test]
fn two_blind_drilled_holes_are_valid() {
    let mut part = block();
    holes_at(
        &mut part,
        HoleSpec::simple(6.0, 10.0),
        &[(10.0, 10.0), (30.0, 20.0)],
    )
    .unwrap();
    let want = BLOCK - 2.0 * (PI * 9.0 * 10.0 + cone(3.0, 118f64.to_radians()));
    check(&part, want, 2e-4);
}

// ------------------------------------------------- owner's follow-ups ---

#[test]
fn hole_centres_are_center_points() {
    let mut part = block();
    holes_at(&mut part, HoleSpec::simple(6.0, 10.0), &[(10.0, 10.0)]).unwrap();
    let ps = &part.sketches[1];
    let (_, p) = ps.sketch.points().next().unwrap();
    assert_eq!(p.style, cadcraft3d::sketch::PointStyle::Center);
    assert_eq!(ps.sketch.center_points().len(), 1);
    // Saved and read back; an old file without the style reads Normal.
    let json = serde_json::to_string(&ps.sketch).unwrap();
    assert!(json.contains("Center"), "{json}");
    let back: cadcraft3d::sketch::Sketch = serde_json::from_str(&json).unwrap();
    assert_eq!(back.center_points().len(), 1);
}

#[test]
fn a_centre_dimensioned_to_an_edge_stays_parametric() {
    // The owner: dimension the centre to model edges, editable later.
    let mut part = block();
    let (i, ids) = part
        .create_hole_sketch(TOP, &[Point3::new(12.0, 9.0, 20.0)])
        .unwrap();
    // Box edge along X at y = 0 on the top face: its end vertices.
    let e = part
        .body
        .edges()
        .find(|(e, x)| {
            let a = part.body.vertex(x.start).unwrap().point;
            let b = part.body.vertex(x.end).unwrap().point;
            part.body.edge_direction(*e).is_some()
                && (a.y.abs() + b.y.abs()) < 1e-9
                && (a.z - 20.0).abs() + (b.z - 20.0).abs() < 1e-9
        })
        .map(|(e, _)| e)
        .unwrap();
    let line = part.project_edge(i, e).unwrap();
    assert_eq!(part.project_edge(i, e).unwrap(), line, "projected once");
    let c = part.sketches[i]
        .sketch
        .add_constraint(cadcraft3d::sketch::Constraint::PointLineDistance {
            point: ids[0],
            line,
            value: 9.0,
        })
        .unwrap();
    let mut def = HoleFeature::new(HoleSpec::simple(6.0, 10.0));
    def.sketch = part.sketches[i].id.clone();
    def.centers = ids.into_iter().map(HoleCenter::Point).collect();
    part.add_hole(def).unwrap();
    assert!((part.holes()[0].origin.y - 9.0).abs() < 1e-6);
    // The dimension is a parameter: change it, the hole follows.
    let name = part.sketches[i].sketch.dim_param(c).name.clone();
    assert!(!name.is_empty());
    part.set_parameter_expr(&name, "15").unwrap();
    part.regenerate().unwrap();
    assert!(
        (part.holes()[0].origin.y - 15.0).abs() < 1e-6,
        "{:?}",
        part.holes()[0].origin
    );
    // The projection follows the model: a longer box keeps it on the edge.
    assert!(
        part.sketches[i].health.is_none(),
        "{:?}",
        part.sketches[i].health
    );
}

#[test]
fn deleting_a_centre_point_removes_its_hole() {
    let mut part = block();
    holes_at(
        &mut part,
        HoleSpec::simple(6.0, 10.0),
        &[(10.0, 10.0), (30.0, 20.0)],
    )
    .unwrap();
    assert_eq!(part.holes().len(), 2);
    let pid = part.sketches[1].sketch.center_points()[0];
    part.sketches[1].sketch.remove_point(pid).unwrap();
    part.regenerate().unwrap();
    assert!(part.feature_health.is_empty(), "{:?}", part.feature_health);
    assert_eq!(part.holes().len(), 1);
}

/// A sketch on a work plane half way up the block, a Ø6 hole through all.
fn mid_plane_hole(symmetric: bool) -> PartDocument {
    use cadcraft3d::part::{PlaneRef, WorkDef, WorkFeature, WorkPlaneDef};
    let mut part = block();
    part.add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
        base: PlaneRef::Face("Extrusion1:Start".into()),
        distance: -10.0,
    })))
    .unwrap();
    let opts = SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    };
    let i = part
        .create_sketch(
            SketchPlacement {
                plane: PlaneRef::Work("WorkPlane1".into()),
                ..Default::default()
            },
            &opts,
        )
        .unwrap();
    let plane = part.sketches[i].sketch.plane.clone();
    let p = part.sketches[i]
        .sketch
        .add_center_point(plane.project(&Point3::new(20.0, 15.0, 10.0)));
    let mut s = HoleSpec::simple(6.0, 10.0);
    s.termination = Termination::ThroughAll;
    s.symmetric = symmetric;
    let mut def = HoleFeature::new(s);
    def.sketch = part.sketches[i].id.clone();
    def.centers = vec![HoleCenter::Point(p)];
    part.add_hole(def).unwrap();
    part
}

#[test]
fn symmetric_through_all_both_ways() {
    let half = mid_plane_hole(false);
    check(&half, BLOCK - PI * 9.0 * 10.0, 3e-4);
    let both = mid_plane_hole(true);
    check(&both, BLOCK - PI * 9.0 * 20.0, 3e-4);
    // Only for a plain Through All hole.
    let mut s = HoleSpec::simple(6.0, 10.0);
    s.symmetric = true;
    assert!(s.validate().is_err());
}

#[test]
fn drill_points_flat_w_angle_v_angle_y() {
    let a = 118f64.to_radians();
    for (dp, want) in [
        (DrillPoint::FlatW, BLOCK - PI * 9.0 * 10.0),
        (DrillPoint::AngleV, BLOCK - PI * 9.0 * 10.0 - cone(3.0, a)),
        // Angle Y: the point is part of the 10 mm depth.
        (
            DrillPoint::AngleY,
            BLOCK - PI * 9.0 * (10.0 - 3.0 / (a / 2.0).tan()) - cone(3.0, a),
        ),
    ] {
        let mut part = block();
        let mut s = HoleSpec::simple(6.0, 10.0);
        s.drill_point = dp;
        holes_at(&mut part, s, &[(20.0, 15.0)]).unwrap();
        check(&part, want, 2e-4);
    }
    assert_eq!(DrillPoint::ALL.len(), 5);
    assert_eq!(DrillPoint::AngleY.label(), "Angle Y");
}

#[test]
fn extend_to_off_needs_the_face_itself() {
    // A slab far off to the side: its top plane (z −5) is under the hole,
    // the face itself is not.
    let mut part = block();
    let opts = SketchOptions {
        auto_project_origin: false,
        ..Default::default()
    };
    let i = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts)
        .unwrap();
    part.sketches[i]
        .sketch
        .add_rectangle_two_point(Point2::new(100.0, 0.0), Point2::new(120.0, 20.0))
        .unwrap();
    let r = part.sketches[i].sketch.regions();
    let reg = r.iter().find(|r| r.interior_point().x > 99.0).unwrap();
    let mut ex = ExtrudeFeature::new(vec![ProfileRef::of(reg, reg.interior_point())], 5.0);
    ex.operation = cadcraft3d::part::Operation::NewSolid;
    ex.direction = cadcraft3d::part::ExtentDirection::Flipped;
    part.add_extrude(i, ex).unwrap();
    let slab_top = part
        .body
        .faces()
        .find(|(_, f)| {
            f.name
                .as_deref()
                .is_some_and(|n| n.starts_with("Extrusion2"))
                && f.mesh
                    .as_ref()
                    .is_some_and(|m| m.positions.iter().all(|p| p.z.abs() < 1e-9))
        })
        .and_then(|(fid, _)| {
            cadcraft3d::part::shell::face_ref(&part.body, fid, Some(Point3::new(110.0, 10.0, 0.0)))
        });
    let Some(face) = slab_top else {
        panic!("no slab face at z 0")
    };
    let mut s = HoleSpec::simple(6.0, 10.0);
    s.termination = Termination::To(face.clone());
    s.drill_point = DrillPoint::Flat;
    let mut on = part.clone();
    holes_at(&mut on, s.clone(), &[(20.0, 15.0)]).unwrap();
    s.extend_to = false;
    let e = holes_at(&mut part, s, &[(20.0, 15.0)]).unwrap_err();
    assert!(e.contains("Extend To"), "{e}");
}

#[test]
fn concentric_to_a_cylindrical_face() {
    // A Ø20 boss on the block; a click on its side: the hole on its axis,
    // from its top, down into it.
    let mut part = block();
    let opts = SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    };
    let j = part
        .create_sketch(SketchPlacement::on_face(TOP), &opts)
        .unwrap();
    let plane = part.sketches[j].sketch.plane.clone();
    let c = plane.project(&Point3::new(20.0, 15.0, 20.0));
    part.sketches[j].sketch.add_circle(c, 10.0).unwrap();
    let r = part.sketches[j].sketch.regions();
    let disc = r.iter().find(|r| r.holes.is_empty()).unwrap();
    let mut ex = ExtrudeFeature::new(vec![ProfileRef::of(disc, disc.interior_point())], 5.0);
    ex.operation = cadcraft3d::part::Operation::Join;
    part.add_extrude(j, ex).unwrap();
    let side = part
        .body
        .faces()
        .find(|(_, f)| {
            f.name
                .as_deref()
                .is_some_and(|n| n.starts_with("Extrusion2:Side"))
        })
        .map(|(fid, _)| fid)
        .unwrap();
    let fr =
        cadcraft3d::part::shell::face_ref(&part.body, side, Some(Point3::new(30.0, 15.0, 24.0)))
            .unwrap();
    let mut def = HoleFeature::new(HoleSpec::simple(8.0, 10.0));
    def.spec.drill_point = DrillPoint::Flat;
    def.centers = vec![HoleCenter::Axis(fr)];
    part.add_hole(def).unwrap();
    check(&part, BLOCK + PI * 100.0 * 5.0 - PI * 16.0 * 10.0, 2e-3);
    let h = &part.holes()[0];
    assert!(
        (h.origin - Point3::new(20.0, 15.0, 25.0)).norm() < 1e-6,
        "{h:?}"
    );
}

// ------------------------------------------------- thread texture (2.9) ---

/// A Ø8 shaft 20 long and its side face (clicked near z 20).
fn shaft_and_side() -> (PartDocument, cadcraft3d::part::shell::FaceRef) {
    let mut shaft = PartDocument::default();
    let i = shaft
        .create_sketch(
            SketchPlacement::on_origin_plane(OriginPlane::XY),
            &SketchOptions::default(),
        )
        .unwrap();
    shaft.sketches[i]
        .sketch
        .add_circle(Point2::new(0.0, 0.0), 4.0)
        .unwrap();
    let r = shaft.sketches[i].sketch.regions();
    let disc = r.iter().find(|r| r.holes.is_empty()).unwrap();
    shaft
        .add_extrude(
            i,
            ExtrudeFeature::new(vec![ProfileRef::of(disc, disc.interior_point())], 20.0),
        )
        .unwrap();
    let side = shaft
        .body
        .faces()
        .find(|(_, f)| f.name.as_deref().is_some_and(|n| n.contains(":Side")))
        .map(|(fid, _)| fid)
        .unwrap();
    let face =
        cadcraft3d::part::shell::face_ref(&shaft.body, side, Some(Point3::new(4.0, 0.0, 19.0)))
            .unwrap();
    (shaft, face)
}

/// The thread texture of a tapped M8 hole: a sleeve on the drill
/// wall (just inside it), only over the 12 mm thread of a 15 mm hole, with
/// 12 / 1.25 = 9.6 bands; the Thread command on a Ø8 shaft: outside the
/// shaft. Display thread in model off: no texture.
#[test]
fn thread_texture_on_hole_and_shaft() {
    use cadcraft3d::part::ThreadBands;
    use cadcraft3d::part::hole::BAND_LIFT;
    let mut part = block();
    let mut t = m8();
    t.full_depth = false;
    t.depth = 12.0;
    let spec = HoleSpec::tapped(t, 15.0);
    let rd = spec.diameter / 2.0;
    holes_at(&mut part, spec, &[(20.0, 15.0)]).unwrap();
    let c = &part.cosmetic_threads[0];
    assert!(c.internal && c.right_hand);
    assert!((c.face_radius - rd).abs() < 1e-9, "{c:?}");
    let b = ThreadBands::of(&part.cosmetic_threads);
    assert!(!b.triangles.is_empty());
    for p in &b.positions {
        let r = ((p.x - 20.0).powi(2) + (p.y - 15.0).powi(2)).sqrt();
        assert!((r - (rd - BAND_LIFT)).abs() < 1e-9, "r {r}");
        assert!(p.z <= 20.0 + 1e-9 && p.z >= 8.0 - 1e-9, "z {}", p.z);
    }
    let lo = b.phase.iter().copied().fold(f32::MAX, f32::min);
    let hi = b.phase.iter().copied().fold(f32::MIN, f32::max);
    // 9.6 bands along the axis plus one turn around.
    assert!((hi - lo - 10.6).abs() < 1e-4, "span {}", hi - lo);

    let (shaft, face) = shaft_and_side();
    let ext =
        ThreadSpec::from_table(&Standards::builtin(), standards::ISO_METRIC, "M8", false).unwrap();
    let mut on = shaft.clone();
    on.add_thread(ThreadFeature::new(face.clone(), ext.clone()))
        .unwrap();
    let c = &on.cosmetic_threads[0];
    assert!(!c.internal && (c.face_radius - 4.0).abs() < 1e-6, "{c:?}");
    let b = ThreadBands::of(&on.cosmetic_threads);
    assert!(!b.triangles.is_empty());
    for (p, n) in b.positions.iter().zip(&b.normals) {
        let r = (p.x * p.x + p.y * p.y).sqrt();
        assert!((r - (4.0 + BAND_LIFT)).abs() < 1e-6, "r {r}");
        // Normals point away from the axis (towards the air).
        assert!(n.x * p.x + n.y * p.y > 0.0);
    }
    // Display thread in model off: nothing drawn.
    let mut th = ThreadFeature::new(face, ext);
    th.display = false;
    let mut off = shaft;
    off.add_thread(th).unwrap();
    assert!(ThreadBands::of(&off.cosmetic_threads).positions.is_empty());
}

/// A 2×2 pattern of a tapped hole: every copy has its cosmetic thread (and
/// so its texture), at the copy's place.
#[test]
fn pattern_copies_carry_the_thread_texture() {
    use cadcraft3d::part::{LineRef, PatternDirection, PatternFeature, PatternKind, PullRef};
    let mut part = block();
    holes_at(&mut part, HoleSpec::tapped(m8(), 15.0), &[(10.0, 8.0)]).unwrap();
    let dir = |axis: usize, count: u32, spacing: Real| {
        let mut d = PatternDirection::new();
        d.dir = Some(PullRef::Line(LineRef::Origin(axis)));
        d.count = count;
        d.spacing = spacing;
        d
    };
    let mut p = PatternFeature::rectangular();
    p.features = vec!["Hole1".into()];
    p.kind = PatternKind::Rectangular {
        first: dir(0, 2, 20.0),
        second: Some(dir(1, 2, 14.0)),
    };
    part.add_pattern(p).unwrap();
    assert_eq!(
        part.cosmetic_threads.len(),
        4,
        "{:?}",
        part.cosmetic_threads
    );
    let mut at: Vec<(i64, i64)> = part
        .cosmetic_threads
        .iter()
        .map(|c| (c.start.x.round() as i64, c.start.y.round() as i64))
        .collect();
    at.sort();
    assert_eq!(at, vec![(10, 8), (10, 22), (30, 8), (30, 22)]);
}
