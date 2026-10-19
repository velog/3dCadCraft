//! Coil (5.3) and Rib / Web (2.7): volumes against analytic values for
//! every method / option, refusals, save/load and cross-feature cases
//! (`docs/spec/5.3-coil.md`, `docs/spec/2.7-rib.md`).

use std::f64::consts::{PI, TAU};

use cadcraft3d::kernel::{Point3, Real, TessellationOptions};
use cadcraft3d::part::work::LineRef;
use cadcraft3d::part::{
    CoilAxis, CoilFeature, CoilMethod, Feature, Operation, OriginPlane, Output, PartDocument,
    ProfileRef, RibDirection, RibExtent, RibFeature, RibSide, SketchOptions, SketchPlacement,
};
use cadcraft3d::sketch::{EntityId, Point2, Style};

fn opts() -> SketchOptions {
    SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    }
}

/// Volume from the display triangles (the exact kernel's own measure
/// tessellates helical walls very finely: seconds).
fn volume(part: &PartDocument) -> Real {
    let o = TessellationOptions::default();
    part.body
        .solids()
        .filter(|(_, s)| !s.open)
        .map(|(sid, _)| {
            let m = part.body.tessellate_solid(sid, &o).unwrap();
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

fn healthy(part: &PartDocument) {
    assert!(part.feature_health.is_empty(), "sick: {:?}", part.feature_health);
}

fn near(got: Real, want: Real, rel: Real, what: &str) {
    assert!(
        (got - want).abs() <= rel * want.abs(),
        "{what}: {got:.3} vs {want:.3} ({:.3} %)",
        100.0 * (got - want) / want
    );
}

/// A part with a sketch on XZ holding a circle of radius `r` at
/// `(x, z)` (sketch coordinates x = X, y = Z). Returns the part and the
/// sketch index.
fn circle_sketch(part: &mut PartDocument, x: Real, z: Real, r: Real) -> usize {
    let k = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XZ), &opts())
        .unwrap();
    part.sketches[k]
        .sketch
        .add_circle(Point2::new(x, z), r)
        .unwrap();
    k
}

fn profiles(part: &PartDocument, k: usize) -> Vec<ProfileRef> {
    let regions = part.sketches[k].sketch.regions();
    regions
        .iter()
        .map(|r| ProfileRef::of(r, r.interior_point()))
        .collect()
}

fn z_coil(part: &PartDocument, k: usize) -> CoilFeature {
    CoilFeature::new(profiles(part, k), CoilAxis::Line(LineRef::Origin(2)))
}

/// Section area × the centre's circle × turns (a meridian section screwed
/// about the axis: the rise does not change the volume).
fn pappus(r: Real, rc: Real, turns: Real) -> Real {
    PI * r * r * TAU * rc * turns
}

#[test]
fn coil_by_each_method() {
    let want = pappus(1.0, 10.0, 3.0);
    for (method, pitch, revs, height) in [
        (CoilMethod::PitchAndRevolution, 5.0, 3.0, 0.0),
        (CoilMethod::RevolutionAndHeight, 0.0, 3.0, 15.0),
        (CoilMethod::PitchAndHeight, 5.0, 0.0, 15.0),
    ] {
        let mut part = PartDocument::default();
        let k = circle_sketch(&mut part, 10.0, 0.0, 1.0);
        let mut c = z_coil(&part, k);
        (c.method, c.pitch, c.revolutions, c.height) = (method, pitch, revs, height);
        let t = std::time::Instant::now();
        part.add_coil(k, c).unwrap();
        let dt = t.elapsed();
        healthy(&part);
        assert!(part.exact, "{method:?}: exact");
        near(volume(&part), want, 0.01, &format!("{method:?}"));
        // The last section is 15 mm up.
        let zmax = part
            .body
            .faces()
            .filter_map(|(_, f)| f.mesh.as_ref())
            .flat_map(|m| m.positions.iter().map(|p| p.z))
            .fold(Real::NEG_INFINITY, Real::max);
        near(zmax, 16.0, 0.01, "top");
        assert!(part.body.find_face("Coil1:Start").is_some());
        assert!(part.body.find_face("Coil1:End").is_some());
        assert!(dt.as_secs_f64() < 10.0, "{method:?} took {dt:?}");
    }
}

#[test]
fn coil_left_and_right_hand() {
    let mut ends = Vec::new();
    for left in [false, true] {
        let mut part = PartDocument::default();
        let k = circle_sketch(&mut part, 10.0, 0.0, 1.0);
        let mut c = z_coil(&part, k);
        (c.pitch, c.revolutions, c.left_hand) = (5.0, 1.25, left);
        part.add_coil(k, c).unwrap();
        healthy(&part);
        near(volume(&part), pappus(1.0, 10.0, 1.25), 0.01, "volume");
        let f = part.body.find_face("Coil1:End").unwrap();
        let m = part.body.face(f).unwrap().mesh.clone().unwrap();
        let c = m.positions.iter().fold(nalgebra::Vector3::zeros(), |a, p| a + p.coords)
            / m.positions.len() as Real;
        ends.push(c);
    }
    // A quarter turn past whole turns: right hand ends at +Y, left at −Y.
    assert!(ends[0].y > 5.0, "right {:?}", ends[0]);
    assert!(ends[1].y < -5.0, "left {:?}", ends[1]);
}

#[test]
fn coil_taper_and_closed_ends() {
    // Taper: the centre radius grows by height·tan(taper).
    let mut part = PartDocument::default();
    let k = circle_sketch(&mut part, 10.0, 0.0, 1.0);
    let mut c = z_coil(&part, k);
    (c.pitch, c.revolutions, c.taper) = (5.0, 2.0, 10f64.to_radians());
    part.add_coil(k, c).unwrap();
    healthy(&part);
    let grow = 10.0 * 10f64.to_radians().tan();
    let mean_r = 10.0 + grow / 2.0;
    near(volume(&part), pappus(1.0, mean_r, 2.0), 0.02, "taper");

    // Closed ends: same volume (turns unchanged), lower height.
    let mut part = PartDocument::default();
    let k = circle_sketch(&mut part, 10.0, 0.0, 1.0);
    let mut c = z_coil(&part, k);
    (c.pitch, c.revolutions) = (5.0, 3.0);
    c.start.closed = true;
    c.end.closed = true;
    part.add_coil(k, c.clone()).unwrap();
    healthy(&part);
    near(volume(&part), pappus(1.0, 10.0, 3.0), 0.02, "closed");
    let (_, _, h) = c.sizes().unwrap();
    // 3 turns, each end 90° flat + 90° transition (half the rise):
    // 1080 − 2·90 − 90 = 810° of full rise, 2.25 pitches.
    near(h, 11.25, 1e-9, "height");
    // Smooth walls (a sweep along the centre's path), not facets.
    assert!(part.body.faces().count() < 20, "{} faces", part.body.faces().count());
}

#[test]
fn coil_refusals_leave_the_part_unchanged() {
    let mut part = PartDocument::default();
    let k = circle_sketch(&mut part, 10.0, 0.0, 1.0);
    let before = part.features.len();
    // The pitch is smaller than the section: turns run into each other.
    let mut c = z_coil(&part, k);
    (c.pitch, c.revolutions) = (1.5, 3.0);
    assert!(part.add_coil(k, c).is_err());
    // An axis through the profile.
    let mut c = z_coil(&part, k);
    c.axis = CoilAxis::Line(LineRef::Origin(0));
    let k2 = circle_sketch(&mut part, 0.0, 0.0, 1.0);
    c.profiles = profiles(&part, k2);
    let e = part.add_coil(k2, c).unwrap_err().to_string();
    assert!(e.contains("intersect"), "{e}");
    // Zero revolutions, ends longer than all turns.
    let mut c = z_coil(&part, k);
    c.revolutions = 0.0;
    assert!(part.add_coil(k, c).is_err());
    let mut c = z_coil(&part, k);
    c.revolutions = 0.3;
    c.start.closed = true;
    assert!(part.add_coil(k, c).is_err());
    assert_eq!(part.features.len(), before);
    assert!(part.body.solids().next().is_none());
}

/// A cylinder Ø20 × 30 on XY (Extrude of a circle), revolve-free.
fn rod(part: &mut PartDocument) {
    let k = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    part.sketches[k]
        .sketch
        .add_circle(Point2::new(0.0, 0.0), 10.0)
        .unwrap();
    let pr = profiles(part, k);
    part.add_extrude(k, cadcraft3d::part::ExtrudeFeature::new(pr, 30.0))
        .unwrap();
}

#[test]
fn coil_cuts_a_thread_into_a_cylinder() {
    let mut part = PartDocument::default();
    rod(&mut part);
    let v0 = volume(&part);
    // A triangle 2 mm deep at the surface (x = 9..10.5), 2 mm wide.
    let k = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XZ), &opts())
        .unwrap();
    let sk = &mut part.sketches[k].sketch;
    let a = sk.add_point(Point2::new(10.5, 4.0));
    let b = sk.add_point(Point2::new(9.0, 5.0));
    let c = sk.add_point(Point2::new(10.5, 6.0));
    sk.add_line_between(a, b).unwrap();
    sk.add_line_between(b, c).unwrap();
    sk.add_line_between(c, a).unwrap();
    let mut coil = z_coil(&part, k);
    (coil.pitch, coil.revolutions, coil.operation) = (3.0, 6.0, Operation::Cut);
    let t = std::time::Instant::now();
    part.add_coil(k, coil).unwrap();
    let dt = t.elapsed();
    healthy(&part);
    let v = volume(&part);
    // The groove inside the rod: the triangle's part at x 9..10 (base
    // 4/3, height 1, centroid at 9.667) over six turns. The zone is cut
    // as facets (chords inside the rod's circle): a few % more comes off.
    let groove = (2.0 / 3.0) * TAU * (9.0 + 2.0 / 3.0) * 6.0;
    let cut = v0 - v;
    assert!(cut > groove * 0.98 && cut < groove * 1.06, "groove {cut} vs {groove}");
    // The end faces and the side outside the zone stay exact.
    assert!(part.body.find_face("Extrusion1:Start").is_some());
    assert!(part.body.find_face("Extrusion1:End").is_some());
    for (sid, _) in part.body.solids() {
        part.body.validate_solid(sid).unwrap();
    }
    assert!(dt.as_secs_f64() < 20.0, "thread cut took {dt:?}");
}

#[test]
fn coil_join_to_a_base_and_surface() {
    // A spring standing on a plate (touching its top face).
    let mut part = PartDocument::default();
    let k = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    part.sketches[k]
        .sketch
        .add_rectangle_two_point(Point2::new(-15.0, -15.0), Point2::new(15.0, 15.0))
        .unwrap();
    let pr = profiles(&part, k);
    let mut base = cadcraft3d::part::ExtrudeFeature::new(pr, 2.0);
    base.direction = cadcraft3d::part::ExtentDirection::Flipped;
    part.add_extrude(k, base).unwrap();
    let v0 = volume(&part);
    let k = circle_sketch(&mut part, 10.0, 0.5, 1.0);
    let mut c = z_coil(&part, k);
    (c.pitch, c.revolutions, c.operation) = (5.0, 2.0, Operation::Join);
    part.add_coil(k, c).unwrap();
    healthy(&part);
    // The coil sinks 0.5 mm into the plate at its start: a little less.
    let v = volume(&part);
    assert!(v > v0 + pappus(1.0, 10.0, 2.0) * 0.9 && v < v0 + pappus(1.0, 10.0, 2.0), "{v}");

    // Surface output: walls only (an open body), the part goes faceted.
    let mut part = PartDocument::default();
    let k = circle_sketch(&mut part, 10.0, 0.0, 1.0);
    let mut c = z_coil(&part, k);
    (c.pitch, c.revolutions, c.output) = (5.0, 1.5, Output::Surface);
    part.add_coil(k, c).unwrap();
    healthy(&part);
    let open: Vec<_> = part.body.solids().filter(|(_, s)| s.open).collect();
    assert_eq!(open.len(), 1, "one surface body");
}

#[test]
fn coil_save_load_and_sketch_axis() {
    let mut part = PartDocument::default();
    let k = circle_sketch(&mut part, 10.0, 0.0, 1.0);
    // A centerline on the Z axis becomes the axis by itself.
    let sk = &mut part.sketches[k].sketch;
    let l = sk.add_line(Point2::new(0.0, -5.0), Point2::new(0.0, 20.0));
    sk.set_style(l, Style::Centerline).unwrap();
    let axis = cadcraft3d::part::coil::auto_axis(&part.sketches[k].sketch).unwrap();
    assert_eq!(axis, CoilAxis::Sketch(l));
    let mut c = CoilFeature::new(profiles(&part, k), axis);
    (c.pitch, c.revolutions, c.flip) = (5.0, 2.0, true);
    part.add_coil(k, c).unwrap();
    healthy(&part);
    // Flipped: it climbs down.
    let zmin = part
        .body
        .faces()
        .filter_map(|(_, f)| f.mesh.as_ref())
        .flat_map(|m| m.positions.iter().map(|p| p.z))
        .fold(Real::INFINITY, Real::min);
    near(zmin, -11.0, 0.01, "flipped bottom");
    let json = serde_json::to_string(&part).unwrap();
    let mut back: PartDocument = serde_json::from_str(&json).unwrap();
    back.regenerate().unwrap();
    healthy(&back);
    near(volume(&back), volume(&part), 1e-6, "reloaded");
    // Edit Feature: more turns.
    let Some(Feature::Coil(mut c)) = back.features.first().cloned() else {
        panic!("no coil");
    };
    c.revolutions = 3.0;
    back.replace_feature(0, Feature::Coil(c)).unwrap();
    near(volume(&back), pappus(1.0, 10.0, 3.0), 0.01, "edited");
}

// ------------------------------------------------------------- ribs ---

/// A sketch on a work plane `d` from an origin plane (along its normal).
fn offset_sketch(part: &mut PartDocument, base: OriginPlane, d: Real) -> usize {
    use cadcraft3d::part::{PlaneRef, WorkDef, WorkFeature, WorkPlaneDef};
    let wp = part
        .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
            base: PlaneRef::Origin(base),
            distance: d,
        })))
        .unwrap();
    part.create_sketch(
        SketchPlacement {
            plane: PlaneRef::Work(wp),
            ..Default::default()
        },
        &opts(),
    )
    .unwrap()
}

/// An edge between a face whose name starts with `a` and one named `b`.
fn edge_between(part: &PartDocument, a: &str, b: &str) -> Option<cadcraft3d::part::EdgeRef> {
    part.body.edges().find_map(|(e, _)| {
        let r = cadcraft3d::part::blend::edge_ref(&part.body, e)?;
        let names = [r.faces[0].as_str(), r.faces[1].as_str()];
        let ok = (names[0].starts_with(a) && names[1].starts_with(b))
            || (names[1].starts_with(a) && names[0].starts_with(b));
        ok.then_some(r)
    })
}

fn volume_before_fillet(part: &PartDocument) -> Real {
    let mut p = part.clone();
    p.build_cache = Default::default();
    let n = p.features.len();
    p.remove_feature(n - 1);
    p.regenerate().unwrap();
    volume(&p)
}

/// An open box 60 × 40 × 20 with 2 mm walls (Extrude of the outline, then
/// a pocket 56 × 36 from the top down to 2 mm above the bottom).
fn open_box(part: &mut PartDocument) {
    let k = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    part.sketches[k]
        .sketch
        .add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(60.0, 40.0))
        .unwrap();
    let pr = profiles(part, k);
    part.add_extrude(k, cadcraft3d::part::ExtrudeFeature::new(pr, 20.0))
        .unwrap();
    let k = offset_sketch(part, OriginPlane::XY, 20.0);
    part.sketches[k]
        .sketch
        .add_rectangle_two_point(Point2::new(2.0, 2.0), Point2::new(58.0, 38.0))
        .unwrap();
    let pr = profiles(part, k);
    let mut cut = cadcraft3d::part::ExtrudeFeature::new(pr, 18.0);
    cut.operation = Operation::Cut;
    cut.direction = cadcraft3d::part::ExtentDirection::Flipped;
    part.add_extrude(k, cut).unwrap();
    healthy(part);
}

/// A sketch on the top of the walls (z = 20) with the given lines.
fn top_sketch(part: &mut PartDocument, lines: &[(Point2, Point2)]) -> (usize, Vec<EntityId>) {
    let k = offset_sketch(part, OriginPlane::XY, 20.0);
    let ids = lines
        .iter()
        .map(|(a, b)| part.sketches[k].sketch.add_line(*a, *b))
        .collect();
    (k, ids)
}

#[test]
fn web_in_an_open_box_to_next() {
    let mut part = PartDocument::default();
    open_box(&mut part);
    let v0 = volume(&part);
    // Across the box, wall to wall (inner faces at x = 2 and 58).
    let (k, ids) = top_sketch(&mut part, &[(Point2::new(2.0, 20.0), Point2::new(58.0, 20.0))]);
    part.add_rib(k, RibFeature::new(ids, 2.0)).unwrap();
    healthy(&part);
    // 56 long, 2 thick, 18 deep (to the floor).
    near(volume(&part) - v0, 56.0 * 2.0 * 18.0, 0.002, "web");
}

#[test]
fn web_thickness_sides_finite_and_extend() {
    for (side, extend, short, depth, want) in [
        (RibSide::Symmetric, true, true, None, 56.0 * 3.0 * 18.0),
        (RibSide::Side1, true, true, None, 56.0 * 3.0 * 18.0),
        (RibSide::Side2, false, false, None, 56.0 * 3.0 * 18.0),
        // Short line, no extension: exactly its length (30).
        (RibSide::Symmetric, false, true, None, 30.0 * 3.0 * 18.0),
        // Finite depth 8.
        (RibSide::Symmetric, true, false, Some(8.0), 56.0 * 3.0 * 8.0),
    ] {
        let mut part = PartDocument::default();
        open_box(&mut part);
        let v0 = volume(&part);
        let (a, b) = if short {
            (Point2::new(15.0, 20.0), Point2::new(45.0, 20.0))
        } else {
            (Point2::new(2.0, 20.0), Point2::new(58.0, 20.0))
        };
        let (k, ids) = top_sketch(&mut part, &[(a, b)]);
        let mut r = RibFeature::new(ids, 3.0);
        (r.side, r.extend) = (side, extend);
        if let Some(d) = depth {
            (r.extent, r.depth) = (RibExtent::Finite, d);
        }
        assert_eq!(
            r.ends_reach(&part.sketches[k].sketch, &part.body),
            !short,
            "ends reach"
        );
        part.add_rib(k, r).unwrap();
        healthy(&part);
        near(
            volume(&part) - v0,
            want,
            0.003,
            &format!("{side:?} extend {extend} short {short}"),
        );
        // Where the thickness went.
        let ys: Vec<Real> = part
            .body
            .faces()
            .filter(|(_, f)| f.name.as_deref().is_some_and(|n| n.starts_with("Rib1:Side:")))
            .filter_map(|(_, f)| f.mesh.as_ref())
            .flat_map(|m| m.positions.iter().map(|p| p.y))
            .collect();
        let (lo, hi) = ys
            .iter()
            .fold((Real::INFINITY, Real::NEG_INFINITY), |(a, b), &y| (a.min(y), b.max(y)));
        let (wl, wh) = match side {
            RibSide::Symmetric => (18.5, 21.5),
            RibSide::Side1 => (20.0, 23.0),
            RibSide::Side2 => (17.0, 20.0),
        };
        assert!((lo - wl).abs() < 1e-3 && (hi - wh).abs() < 1e-3, "{side:?}: {lo}..{hi}");
    }
}

#[test]
fn web_network_of_three() {
    let mut part = PartDocument::default();
    open_box(&mut part);
    let v0 = volume(&part);
    let (k, ids) = top_sketch(
        &mut part,
        &[
            (Point2::new(2.0, 20.0), Point2::new(58.0, 20.0)),
            (Point2::new(20.0, 2.0), Point2::new(20.0, 38.0)),
            (Point2::new(40.0, 2.0), Point2::new(40.0, 38.0)),
        ],
    );
    part.add_rib(k, RibFeature::new(ids, 2.0)).unwrap();
    healthy(&part);
    // 56·2 + 2·36·2 − 2 crossings of 2×2, all 18 deep.
    let want = (56.0 * 2.0 + 2.0 * 36.0 * 2.0 - 2.0 * 4.0) * 18.0;
    near(volume(&part) - v0, want, 0.003, "network");
}

#[test]
fn web_taper_and_draft_hold() {
    let a = 2f64.to_radians();
    for (taper, draft, hold, want_top, want_root) in [
        (a, 0.0, cadcraft3d::part::DraftHold::Top, 2.0, 2.0 + 2.0 * 18.0 * a.tan()),
        (0.0, a, cadcraft3d::part::DraftHold::Top, 2.0, 2.0 + 2.0 * 18.0 * a.tan()),
        (0.0, a, cadcraft3d::part::DraftHold::Root, 2.0 - 2.0 * 18.0 * a.tan(), 2.0),
    ] {
        let mut part = PartDocument::default();
        open_box(&mut part);
        let v0 = volume(&part);
        let (k, ids) = top_sketch(&mut part, &[(Point2::new(2.0, 20.0), Point2::new(58.0, 20.0))]);
        let mut r = RibFeature::new(ids, 2.0);
        (r.taper, r.draft, r.hold) = (taper, draft, hold);
        part.add_rib(k, r).unwrap();
        healthy(&part);
        // A trapezoid section (top → root) over 56 mm; the drafted ends
        // inside the walls add a sliver.
        let want = (want_top + want_root) / 2.0 * 18.0 * 56.0;
        near(volume(&part) - v0, want, 0.01, &format!("{hold:?}"));
    }
}

#[test]
fn web_bosses() {
    let mut part = PartDocument::default();
    open_box(&mut part);
    let v0 = volume(&part);
    let (k, ids) = top_sketch(&mut part, &[(Point2::new(2.0, 20.0), Point2::new(58.0, 20.0))]);
    let c1 = part.sketches[k].sketch.add_point(Point2::new(20.0, 20.0));
    let c2 = part.sketches[k].sketch.add_point(Point2::new(40.0, 20.0));
    let mut r = RibFeature::new(ids, 2.0);
    r.boss.centers = vec![c1, c2];
    r.boss.diameter = 6.0;
    part.add_rib(k, r).unwrap();
    healthy(&part);
    // Two Ø6 bosses over the 2 mm web: each adds the circle less the
    // web's band through it.
    let seg = {
        // Area of a Ø6 circle outside a 2 mm band through its centre.
        let r: Real = 3.0;
        let h: Real = 1.0;
        let band = 2.0 * (h * (r * r - h * h).sqrt() + r * r * (h / r).asin());
        PI * r * r - band
    };
    near(
        volume(&part) - v0,
        (56.0 * 2.0 + 2.0 * seg) * 18.0,
        0.005,
        "bosses",
    );
    assert!(part.body.find_face("Rib1:Boss1:Side:Wall").is_some());
}

/// An L bracket: a 40 × 30 plate (XY) with a 40 × 30 wall at x = 0..3 on
/// it; base thickness 3. Sketch on the middle plane y = 15 (XZ offset).
fn bracket(part: &mut PartDocument) {
    let k = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XZ), &opts())
        .unwrap();
    let sk = &mut part.sketches[k].sketch;
    let pts = [(0.0, 0.0), (40.0, 0.0), (40.0, 3.0), (3.0, 3.0), (3.0, 40.0), (0.0, 40.0)];
    let ids: Vec<_> = pts
        .iter()
        .map(|&(x, z)| sk.add_point(Point2::new(x, z)))
        .collect();
    for i in 0..ids.len() {
        sk.add_line_between(ids[i], ids[(i + 1) % ids.len()]).unwrap();
    }
    let pr = profiles(part, k);
    // XZ normal is −Y: a flipped extrusion goes to +Y.
    let mut e = cadcraft3d::part::ExtrudeFeature::new(pr, 30.0);
    e.direction = cadcraft3d::part::ExtentDirection::Flipped;
    part.add_extrude(k, e).unwrap();
    healthy(part);
}

#[test]
fn rib_parallel_in_a_bracket_corner() {
    for (extent, want_area) in [
        // To Next: the triangle between the diagonal and the inner corner
        // (3,3): legs 20 → 200.
        (RibExtent::ToNext, 200.0),
    ] {
        let mut part = PartDocument::default();
        bracket(&mut part);
        let v0 = volume(&part);
        // XZ's normal is −Y: y = 15 is 15 against it.
        let k = offset_sketch(&mut part, OriginPlane::XZ, -15.0);
        let sk = &mut part.sketches[k].sketch;
        let l = sk.add_line(Point2::new(3.0, 23.0), Point2::new(23.0, 3.0));
        let mut r = RibFeature::new(vec![l], 4.0);
        (r.direction, r.extent) = (RibDirection::Parallel, extent);
        part.add_rib(k, r).unwrap();
        healthy(&part);
        near(volume(&part) - v0, want_area * 4.0, 0.002, "gusset");
    }
}

#[test]
fn rib_refusals_leave_the_part_unchanged() {
    let mut part = PartDocument::default();
    open_box(&mut part);
    let v0 = volume(&part);
    let ribs = |p: &PartDocument| p.features.iter().filter(|f| matches!(f, Feature::Rib(_))).count();
    let n = ribs(&part);
    // A line outside the box: To Next finds no face.
    let (k, ids) = top_sketch(&mut part, &[(Point2::new(80.0, 0.0), Point2::new(90.0, 0.0))]);
    let mut r = RibFeature::new(ids.clone(), 2.0);
    r.extend = false;
    assert!(part.add_rib(k, r).is_err());
    // Zero thickness.
    assert!(part.add_rib(k, RibFeature::new(ids, 0.0)).is_err());
    // A closed profile parallel to the plane.
    let (k, _) = top_sketch(&mut part, &[]);
    part.sketches[k]
        .sketch
        .add_rectangle_two_point(Point2::new(10.0, 10.0), Point2::new(20.0, 20.0))
        .unwrap();
    let curves = cadcraft3d::part::rib::default_curves(&part.sketches[k]);
    let mut r = RibFeature::new(curves, 2.0);
    r.direction = RibDirection::Parallel;
    assert!(part.add_rib(k, r).is_err());
    assert_eq!(ribs(&part), n);
    near(volume(&part), v0, 1e-9, "unchanged");
}

#[test]
fn rib_save_load_and_edit() {
    let mut part = PartDocument::default();
    open_box(&mut part);
    let v0 = volume(&part);
    let (k, ids) = top_sketch(&mut part, &[(Point2::new(2.0, 20.0), Point2::new(58.0, 20.0))]);
    part.add_rib(k, RibFeature::new(ids, 2.0)).unwrap();
    let json = serde_json::to_string(&part).unwrap();
    let mut back: PartDocument = serde_json::from_str(&json).unwrap();
    back.regenerate().unwrap();
    healthy(&back);
    near(volume(&back), volume(&part), 1e-6, "reloaded");
    let k = back.features.len() - 1;
    let Some(Feature::Rib(mut r)) = back.features.get(k).cloned() else {
        panic!("no rib");
    };
    r.thickness = 4.0;
    back.replace_feature(k, Feature::Rib(r)).unwrap();
    near(volume(&back) - v0, 56.0 * 4.0 * 18.0, 0.002, "edited");
}

// --------------------------------------------------------- cross tests ---

#[test]
fn rib_in_a_shelled_box() {
    use cadcraft3d::part::{ShellFeature, shell::face_ref};
    let mut part = PartDocument::with_box("Box1", [60.0, 40.0, 20.0]).unwrap();
    let top = part.body.find_face("Box1:Top").unwrap();
    let mut sh = ShellFeature::new(2.0);
    sh.remove.push(face_ref(&part.body, top, None).unwrap());
    part.add_shell(sh).unwrap();
    healthy(&part);
    let v0 = volume(&part);
    let (k, ids) = top_sketch(
        &mut part,
        &[
            (Point2::new(30.0, 5.0), Point2::new(30.0, 35.0)),
            (Point2::new(10.0, 20.0), Point2::new(50.0, 20.0)),
        ],
    );
    part.add_rib(k, RibFeature::new(ids, 2.0)).unwrap();
    healthy(&part);
    // Extended to the inner walls: 36 and 56 long, one 2×2 crossing, 18 deep.
    near(
        volume(&part) - v0,
        (36.0 * 2.0 + 56.0 * 2.0 - 4.0) * 18.0,
        0.003,
        "ribs in shell",
    );
}

#[test]
fn rib_then_fillet_and_pattern() {
    use cadcraft3d::part::{BlendFeature, PatternFeature};
    let mut part = PartDocument::default();
    open_box(&mut part);
    let (k, ids) = top_sketch(&mut part, &[(Point2::new(10.0, 2.0), Point2::new(10.0, 38.0))]);
    part.add_rib(k, RibFeature::new(ids, 2.0)).unwrap();
    healthy(&part);
    let v1 = volume(&part);
    // A rectangular pattern of the rib: 3 along X, 15 apart.
    let mut p = PatternFeature::rectangular();
    p.features = vec!["Rib1".into()];
    if let cadcraft3d::part::PatternKind::Rectangular { first, .. } = &mut p.kind {
        first.count = 3;
        first.spacing = 15.0;
        first.dir = Some(cadcraft3d::part::PullRef::Line(LineRef::Origin(0)));
    }
    let id = part.add_pattern(p);
    if let Err(e) = &id {
        panic!("pattern: {e}");
    }
    healthy(&part);
    near(volume(&part) - v1, 2.0 * 36.0 * 2.0 * 18.0, 0.003, "pattern");
    // Fillet the top edge of the rib (between its side and its top).
    // The root of the rib: its side meeting the floor (a concave round).
    let e = edge_between(&part, "Rib1:Side:Left", "Extrusion2").expect("rib root edge");
    part.add_blend(BlendFeature::fillet(vec![e], 0.5)).unwrap();
    healthy(&part);
    assert!(volume(&part) > volume_before_fillet(&part), "a root round adds material");
}

#[test]
fn coil_then_fillet_on_the_base() {
    use cadcraft3d::part::BlendFeature;
    let mut part = PartDocument::default();
    rod(&mut part);
    let k = circle_sketch(&mut part, 13.0, 15.0, 1.0);
    let mut c = z_coil(&part, k);
    (c.pitch, c.revolutions, c.operation) = (4.0, 1.0, Operation::NewSolid);
    part.add_coil(k, c).unwrap();
    healthy(&part);
    // Round the rod's top edge after the coil.
    let e = edge_between(&part, "Extrusion1:Side", "Extrusion1:End").expect("rod top edge");
    part.add_blend(BlendFeature::fillet(vec![e], 1.0)).unwrap();
    healthy(&part);
}

/// SolidCraft #793: a coil split by a plane. Here: a box cut through half
/// of a coil — the rest must stay a valid solid.
#[test]
fn coil_split_by_a_plane_solidcraft_793() {
    let mut part = PartDocument::default();
    let k = circle_sketch(&mut part, 10.0, 0.0, 1.0);
    let mut c = z_coil(&part, k);
    (c.pitch, c.revolutions) = (5.0, 2.0);
    part.add_coil(k, c).unwrap();
    let v0 = volume(&part);
    let k = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    part.sketches[k]
        .sketch
        .add_rectangle_two_point(Point2::new(0.0, -20.0), Point2::new(20.0, 20.0))
        .unwrap();
    let pr = profiles(&part, k);
    let mut cut = cadcraft3d::part::ExtrudeFeature::new(pr, 30.0);
    cut.operation = Operation::Cut;
    cut.direction = cadcraft3d::part::ExtentDirection::Symmetric;
    part.add_extrude(k, cut).unwrap();
    healthy(&part);
    near(volume(&part), v0 / 2.0, 0.02, "half a coil");
    for (sid, _) in part.body.solids() {
        part.body.validate_solid(sid).unwrap();
    }
}

#[test]
fn coil_defaults_and_fields() {
    let c = CoilFeature::new(Vec::new(), CoilAxis::Line(LineRef::Origin(2)));
    assert_eq!(c.method, CoilMethod::PitchAndRevolution);
    assert!(!c.left_hand && !c.start.closed && !c.end.closed);
    let names: Vec<_> = c.fields().into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, ["pitch", "revolutions", "taper"]);
    let mut c = c;
    c.method = CoilMethod::PitchAndHeight;
    c.end.closed = true;
    let names: Vec<_> = c.fields().into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, ["pitch", "height", "taper", "end_transition", "end_flat"]);
    let _ = Point3::origin();
}
