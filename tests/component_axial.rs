//! Library fasteners built without a Boolean (`BodyOp::Axial`, spec 6.3 §6.0b):
//! the same geometry as the old Extrude / Revolve templates and as the formula,
//! valid, named alike at every size, and usable by later features.

use std::f64::consts::PI;

use cadcraft3d::components::samples::{Detail, Kind, Sample, all};
use cadcraft3d::document::{DocumentData, Project};
use cadcraft3d::part::PartDocument;
use cadcraft3d::part::frame::{Loop, draw_loop};
use cadcraft3d::part::physical::{MassProps, physical_properties};
use cadcraft3d::part::{
    AxisRef, ExtentDirection, ExtrudeFeature, Operation, OriginPlane, ProfileRef, RevolveFeature,
    SketchOptions, SketchPlacement,
};
use cadcraft3d::sketch::Point2;

/// The display triangles are inscribed ~0.01 mm inside a round: a tolerance by
/// the part's size.
fn close(what: &str, measured: f64, expected: f64, size: f64) {
    let tol = expected.abs() * (2e-3 + 0.02 / size.max(1.0));
    assert!(
        (measured - expected).abs() <= tol,
        "{what}: {measured} vs {expected} (tol {tol})"
    );
}

fn props(p: &PartDocument) -> MassProps {
    physical_properties(p, Default::default()).unwrap().total
}

/// First, middle and last member of a family.
fn members(family: &str) -> Vec<Sample> {
    let v: Vec<Sample> = all().into_iter().filter(|s| s.family == family).collect();
    assert!(!v.is_empty(), "no family {family}");
    vec![v[0].clone(), v[v.len() / 2].clone(), v[v.len() - 1].clone()]
}

/// Volume, area and z moment of a stack of solids of revolution or prisms.
#[derive(Default)]
struct Tally {
    v: f64,
    a: f64,
    mz: f64,
}

impl Tally {
    /// A frustum from (r0, z0) to (r1, z1), its volume, its side's area.
    fn frustum(&mut self, r0: f64, z0: f64, r1: f64, z1: f64) {
        let h = z1 - z0;
        let v = PI * h.abs() / 3.0 * (r0 * r0 + r0 * r1 + r1 * r1);
        let zc = z0
            + h * (r0 * r0 + 2.0 * r0 * r1 + 3.0 * r1 * r1) / (4.0 * (r0 * r0 + r0 * r1 + r1 * r1));
        self.v += v;
        self.mz += v * zc;
        self.a += PI * (r0 + r1) * ((r1 - r0).powi(2) + h * h).sqrt();
    }
    /// A hexagon prism between two heights, its six sides' area.
    fn hexagon(&mut self, s: f64, z0: f64, z1: f64, sign: f64) {
        let v = hex_area(s) * (z1 - z0).abs() * sign;
        self.v += v;
        self.mz += v * (z0 + z1) / 2.0;
        self.a += 6.0 * s / 3f64.sqrt() * (z1 - z0).abs();
    }
}

fn hex_area(s: f64) -> f64 {
    3f64.sqrt() / 2.0 * s * s
}

fn check(s: &Sample, p: &PartDocument, t: &Tally) {
    let m = props(p);
    let size = t.v.cbrt();
    let name = s.designation();
    close(&format!("{name} volume"), m.volume, t.v, size);
    close(&format!("{name} area"), m.area, t.a, size);
    let z = t.mz / t.v;
    assert!(
        (m.center.z - z).abs() < 0.01 + size * 1e-3,
        "{name} centre {} vs {z}",
        m.center.z
    );
    assert!(m.center.x.abs() < 1e-3 + size * 1e-3 && m.center.y.abs() < 1e-3 + size * 1e-3);
}

fn valid(p: &PartDocument) {
    assert!(p.exact, "fell back to the faceted kernel");
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    assert_eq!(p.body.solids().count(), 1);
    for (id, _) in p.body.solids() {
        p.body.validate_solid(id).unwrap();
    }
}

/// The thread diameters come from the tables; the kinds hold the nominal ones.
fn major(p: &PartDocument) -> f64 {
    2.0 * p.cosmetic_threads[0].face_radius
}

fn minor_bore(p: &PartDocument) -> f64 {
    2.0 * p.cosmetic_threads[0].face_radius
}

#[test]
fn a_hex_bolt_is_what_its_formula_says() {
    for s in members("ISO 4017") {
        let p = s.part().unwrap();
        valid(&p);
        let Kind::HexBolt(_, l, a, k) = s.kind else {
            panic!("{}", s.designation())
        };
        let d = major(&p);
        let (r, c) = (d / 2.0, d / 10.0);
        let mut t = Tally::default();
        t.frustum(r - c, -l, r, -l + c);
        t.frustum(r, -l + c, r, 0.0);
        t.hexagon(a, 0.0, k, 1.0);
        // End disc, seat (hexagon less shank), top.
        t.a += PI * (r - c) * (r - c) + (hex_area(a) - PI * r * r) + hex_area(a);
        check(&s, &p, &t);
        // Its cosmetic thread found the shank.
        assert!(
            p.cosmetic_threads[0].length > 0.0 && !p.cosmetic_threads[0].internal,
            "{}",
            s.designation()
        );
    }
}

#[test]
fn a_hex_nut_is_what_its_formula_says() {
    for s in members("ISO 4032") {
        let p = s.part().unwrap();
        valid(&p);
        let Kind::HexNut(_, a, m) = s.kind else {
            panic!("{}", s.designation())
        };
        let r = minor_bore(&p) / 2.0;
        let mut t = Tally::default();
        t.hexagon(a, 0.0, m, 1.0);
        let bore = PI * r * r * m;
        t.v -= bore;
        t.mz -= bore * m / 2.0;
        t.a += 2.0 * PI * r * m + 2.0 * (hex_area(a) - PI * r * r);
        check(&s, &p, &t);
    }
}

#[test]
fn a_socket_screw_is_what_its_formula_says() {
    for s in members("ISO 4762") {
        let p = s.part().unwrap();
        valid(&p);
        let Kind::SocketScrew(_, l, dk, k, a, depth) = s.kind else {
            panic!("{}", s.designation())
        };
        let d = major(&p);
        let (r, rk, c, e) = (d / 2.0, dk / 2.0, d / 10.0, dk / 25.0);
        let mut t = Tally::default();
        t.frustum(r - c, -l, r, -l + c);
        t.frustum(r, -l + c, r, 0.0);
        t.frustum(rk, 0.0, rk, k - e);
        t.frustum(rk, k - e, rk - e, k);
        let (floor, sv) = (k - depth, hex_area(a) * depth);
        t.v -= sv;
        t.mz -= sv * (k + floor) / 2.0;
        t.a += 6.0 * a / 3f64.sqrt() * depth;
        // End disc, seat ring, top ring less socket, socket floor.
        t.a += PI * (r - c) * (r - c)
            + PI * (rk * rk - r * r)
            + (PI * (rk - e) * (rk - e) - hex_area(a))
            + hex_area(a);
        check(&s, &p, &t);
    }
}

fn options() -> SketchOptions {
    SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    }
}

/// `outline` on XZ revolved about Z.
fn revolve(part: &mut PartDocument, outline: &[(f64, f64)], op: Operation) {
    let i = part
        .create_sketch(
            SketchPlacement::on_origin_plane(OriginPlane::XZ),
            &options(),
        )
        .unwrap();
    let v = outline
        .iter()
        .map(|&(r, z)| (Point2::new(r, z), None))
        .collect();
    draw_loop(&mut part.sketches[i].sketch, Loop::Path(v)).unwrap();
    let regions = part.sketches[i].sketch.regions();
    let r = regions.first().unwrap();
    let mut f = RevolveFeature::new(
        vec![ProfileRef::of(r, r.interior_point())],
        AxisRef::Origin(2),
    );
    f.operation = op;
    part.add_revolve(i, f).unwrap();
}

fn hexagon(s: f64) -> Loop {
    let r = s / 3f64.sqrt();
    Loop::Path(
        (0..6)
            .map(|k| {
                let a = k as f64 * std::f64::consts::FRAC_PI_3;
                (Point2::new(r * a.cos(), r * a.sin()), None)
            })
            .collect(),
    )
}

/// The old templates' way: a hexagon on XY at height 0, joined to the rest.
fn hex_extrude(part: &mut PartDocument, s: f64, h: f64, op: Operation) {
    let i = part
        .create_sketch(
            SketchPlacement::on_origin_plane(OriginPlane::XY),
            &options(),
        )
        .unwrap();
    draw_loop(&mut part.sketches[i].sketch, hexagon(s)).unwrap();
    let sk = &part.sketches[i].sketch;
    let flip = sk.plane.normal.z < 0.0;
    let q = Point2::new(0.0, 0.0);
    let regions = sk.regions();
    let r = regions.iter().find(|r| r.contains(q)).unwrap();
    let mut e = ExtrudeFeature::new(vec![ProfileRef::of(r, q)], h);
    e.operation = op;
    if flip {
        e.direction = ExtentDirection::Flipped;
    }
    part.add_extrude(i, e).unwrap();
}

/// The direct bolt against the Boolean of a hexagon and a shank.
#[test]
fn a_hex_bolt_matches_the_boolean_template() {
    let s = members("ISO 4017")[1].clone();
    let new = s.part().unwrap();
    let Kind::HexBolt(_, l, a, k) = s.kind else {
        panic!()
    };
    let d = major(&new);
    let (r, c) = (d / 2.0, d / 10.0);
    let mut old = PartDocument {
        exact_only: true,
        ..Default::default()
    };
    hex_extrude(&mut old, a, k, Operation::NewSolid);
    revolve(
        &mut old,
        &[
            (0.0, k / 2.0),
            (r, k / 2.0),
            (r, -l + c),
            (r - c, -l),
            (0.0, -l),
        ],
        Operation::Join,
    );
    let (m, o) = (props(&new), props(&old));
    assert!(
        (m.volume - o.volume).abs() < o.volume * 1e-3,
        "{} vs {}",
        m.volume,
        o.volume
    );
    assert!(
        (m.area - o.area).abs() < o.area * 1e-3,
        "{} vs {}",
        m.area,
        o.area
    );
    assert!((m.center.z - o.center.z).abs() < 0.01);
    let (a0, b0) = (
        new.body.bounding_box().unwrap(),
        old.body.bounding_box().unwrap(),
    );
    assert!((a0.0 - b0.0).norm() < 1e-6 && (a0.1 - b0.1).norm() < 1e-6);
    // One visible seat, one hexagon's flats: as many named walls.
    let count = |p: &PartDocument, plane: bool| {
        p.body
            .faces()
            .filter(|(_, f)| {
                matches!(
                    p.body.surface(f.surface),
                    Some(cadcraft3d::kernel::Surface::Plane(_))
                ) == plane
            })
            .count()
    };
    assert_eq!(count(&new, true), count(&old, true), "planar faces");
}

/// Every face has a name made of the feature and a role, and all sizes of a
/// family have the same names (Change Size keeps what refers to them).
#[test]
fn every_size_of_a_family_has_the_same_face_names() {
    for family in ["ISO 4017", "ISO 4014", "ISO 4032", "ISO 4762", "DIN 933"] {
        let names = |s: &Sample, d: Detail| {
            let p = s.part_at(d).unwrap();
            let mut v: Vec<String> = p
                .body
                .faces()
                .map(|(_, f)| f.name.clone().expect("a face without a name"))
                .collect();
            v.sort();
            v.dedup();
            v
        };
        for d in [Detail::Standard, Detail::Full] {
            let m = members(family);
            let first = names(&m[0], d);
            assert!(first.iter().all(|n| n.starts_with("Axial1:")), "{first:?}");
            assert!(first.len() >= 9, "{first:?}");
            for s in &m[1..] {
                assert_eq!(names(s, d), first, "{family} {:?}", s.designation());
            }
        }
    }
}

#[test]
fn a_saved_fastener_reads_back_the_same() {
    let dir = std::env::temp_dir().join(format!("cc-axial-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let every = all();
    for (k, name) in ["ISO 4017 M8 x 30", "ISO 4032 M8", "ISO 4762 M8 x 30"]
        .iter()
        .enumerate()
    {
        let s = every.iter().find(|s| s.designation() == *name).unwrap();
        for d in [Detail::Standard, Detail::Full] {
            let p = s.part_at(d).unwrap();
            let before = props(&p);
            let path = dir.join(format!("{k}{}.3dcadcraft", d.label()));
            let mut project = Project::new("t");
            project.add_document("t", DocumentData::Part(p));
            project.save(&path).unwrap();
            let loaded = Project::load(&path).unwrap();
            let DocumentData::Part(q) = &loaded.documents()[0].data else {
                panic!()
            };
            // The body as the file holds it, no meshes rebuilt by us.
            for (id, _) in q.body.solids() {
                q.body.validate_solid(id).unwrap();
            }
            let mut q = q.clone();
            let shown = props(&q);
            assert!(
                (shown.volume - before.volume).abs() < before.volume * 1e-3,
                "{name}: file {} vs {}",
                shown.volume,
                before.volume
            );
            q.regenerate().unwrap();
            valid(&q);
            let again = props(&q);
            assert!(
                (again.volume - before.volume).abs() < before.volume * 1e-9,
                "{name}: regenerated {} vs {}",
                again.volume,
                before.volume
            );
            assert_eq!(q.cosmetic_threads.len(), 1);
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A hole across the shank: the body works with the kernel's Boolean like any other.
#[test]
fn a_fastener_takes_later_features() {
    let s = all()
        .into_iter()
        .find(|s| s.designation() == "ISO 4017 M8 x 30")
        .unwrap();
    for d in [Detail::Standard, Detail::Full] {
        let mut p = s.part_at(d).unwrap();
        let before = props(&p).volume;
        let i = p
            .create_sketch(
                SketchPlacement::on_origin_plane(OriginPlane::XZ),
                &options(),
            )
            .unwrap();
        draw_loop(
            &mut p.sketches[i].sketch,
            Loop::Circle(Point2::new(0.0, -15.0), 1.5, false),
        )
        .unwrap();
        let regions = p.sketches[i].sketch.regions();
        let r = regions.first().unwrap();
        let mut e = ExtrudeFeature::new(vec![ProfileRef::of(r, Point2::new(0.0, -15.0))], 20.0);
        e.operation = Operation::Cut;
        e.direction = ExtentDirection::Symmetric;
        p.add_extrude(i, e).unwrap();
        assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
        let after = props(&p).volume;
        assert!(after < before - 50.0, "{after} vs {before}");
        for (id, _) in p.body.solids() {
            p.body.validate_solid(id).unwrap();
        }
    }
}

/// STEP carries the exact faces (the cones and hyperbolas of Full too).
#[test]
fn a_fastener_exports_to_step_with_its_volume() {
    use cadcraft3d::kernel::exact;
    let every = all();
    for name in ["ISO 4017 M8 x 30", "ISO 4032 M8", "ISO 4762 M8 x 30"] {
        let s = every.iter().find(|s| s.designation() == name).unwrap();
        for d in [Detail::Standard, Detail::Full] {
            let p = s.part_at(d).unwrap();
            let v = props(&p).volume;
            let bodies = &p.exact_bodies.0;
            let text = exact::step_text(bodies, name).unwrap();
            let back = exact::step_volumes(&text).unwrap();
            assert_eq!(back.len(), 1, "{name}");
            assert!(
                (back[0] - v).abs() < v * 5e-3,
                "{name} {d:?}: STEP {} vs {v}",
                back[0]
            );
        }
    }
}

/// What one 30° cone from a circle of 0.95 s takes off a hexagon across flats
/// `a`: the depth (rho - r0) tan 30° over the flats' polar outline. (A grid over
/// the section loses this small number in the hexagon's own area.)
fn chamfer_removes(a: f64) -> f64 {
    let (r0, t, apothem) = (0.475 * a, 30f64.to_radians().tan(), a / 2.0);
    let f = |p: f64| t * (p * p * p / 3.0 - r0 * p * p / 2.0);
    let n = 20_000;
    let step = PI / 6.0 / n as f64;
    let half_sector: f64 = (0..n)
        .map(|i| f(apothem / ((i as f64 + 0.5) * step).cos()) - f(r0))
        .sum::<f64>()
        * step;
    12.0 * half_sector
}

#[test]
fn a_full_hexagon_has_exact_chamfers() {
    for s in members("ISO 4032") {
        let p = s.part_at(Detail::Full).unwrap();
        valid(&p);
        let Kind::HexNut(_, a, m) = s.kind else {
            panic!()
        };
        let r = minor_bore(&p) / 2.0;
        let expected = (hex_area(a) - PI * r * r) * m - 2.0 * chamfer_removes(a);
        let v = props(&p).volume;
        close(
            &format!("{} volume", s.designation()),
            v,
            expected,
            expected.cbrt(),
        );
        // What the chamfers remove, on a mesh fine enough for the smallest
        // nut (the display's 0.01 mm chord is ~2 % of its chamfer's size).
        let fine = |p: &PartDocument| p.exact_bodies.0[0].volume_at(2e-4).unwrap();
        let standard = s.part().unwrap();
        let removed = fine(&standard) - fine(&p);
        let integrated = 2.0 * chamfer_removes(a);
        assert!(
            (removed - integrated).abs() < integrated * 2e-3,
            "{}: removed {removed} vs {integrated}",
            s.designation()
        );
    }
    for s in members("ISO 4017") {
        let p = s.part_at(Detail::Full).unwrap();
        valid(&p);
        let Kind::HexBolt(_, l, a, k) = s.kind else {
            panic!()
        };
        let d = major(&p);
        let (r, c) = (d / 2.0, d / 10.0);
        let mut t = Tally::default();
        t.frustum(r - c, -l, r, -l + c);
        t.frustum(r, -l + c, r, 0.0);
        let expected = t.v + hex_area(a) * k - chamfer_removes(a);
        let v = props(&p).volume;
        close(
            &format!("{} volume", s.designation()),
            v,
            expected,
            expected.cbrt(),
        );
    }
}

/// The shank's chamfer leaves a cylinder shorter than the bolt: a thread longer
/// than that cylinder runs its whole length (ISO 4014 M36 x 80: b = 78).
#[test]
fn a_thread_longer_than_the_cylinder_takes_all_of_it() {
    let every = all();
    for name in ["ISO 4014 M36 x 80", "DIN 931 M36 x 80"] {
        let s = every.iter().find(|s| s.designation() == name).unwrap();
        for d in [Detail::Standard, Detail::Full] {
            let p = s.part_at(d).unwrap();
            valid(&p);
            assert_eq!(p.cosmetic_threads.len(), 1, "{name}");
        }
    }
}

impl Tally {
    /// The ball's zone between heights z0 < z1, the ball's centre at `zc`, radius `rs`.
    fn sphere_zone(&mut self, zc: f64, rs: f64, z0: f64, z1: f64) {
        let (a, b) = (z0 - zc, z1 - zc);
        let v = PI * (rs * rs * (b - a) - (b * b * b - a * a * a) / 3.0);
        // ∫ z·π(rs² − (z − zc)²) dz about the centre, moved to the origin.
        let m = PI * (rs * rs * (b * b - a * a) / 2.0 - (b.powi(4) - a.powi(4)) / 4.0);
        self.v += v;
        self.mz += m + v * zc;
        self.a += 2.0 * PI * rs * (b - a);
    }
    /// A cylinder bored out of what is there.
    fn bore(&mut self, r: f64, z0: f64, z1: f64) {
        let v = PI * r * r * (z1 - z0);
        self.v -= v;
        self.mz -= v * (z0 + z1) / 2.0;
        self.a += 2.0 * PI * r * (z1 - z0);
    }
    /// A hexagonal socket taken out from z0 down to z1.
    fn socket(&mut self, a: f64, z0: f64, z1: f64) {
        let v = hex_area(a) * (z0 - z1);
        self.v -= v;
        self.mz -= v * (z0 + z1) / 2.0;
        self.a += 6.0 * a / 3f64.sqrt() * (z0 - z1);
    }
    /// A flat ring or disc's area.
    fn flat(&mut self, area: f64) {
        self.a += area;
    }
}

fn disc(r: f64) -> f64 {
    PI * r * r
}

#[test]
fn the_other_fasteners_are_what_their_formulas_say() {
    let mut seen = 0;
    for s in all() {
        if !matches!(
            s.kind,
            Kind::Countersunk(..)
                | Kind::ButtonHead(..)
                | Kind::SetScrew(..)
                | Kind::FlangeBolt(..)
                | Kind::FlangeNut(..)
                | Kind::CapNut(..)
        ) {
            continue;
        }
        seen += 1;
        if seen % 7 != 0 {
            continue;
        }
        let p = s
            .part()
            .unwrap_or_else(|e| panic!("{}: {e}", s.designation()));
        valid(&p);
        let mut t = Tally::default();
        match s.kind {
            Kind::Countersunk(_, l, dk, k, a) => {
                let d = major(&p);
                let (r, c) = (d / 2.0, d / 10.0);
                t.frustum(r - c, -l, r, -l + c);
                t.frustum(r, -l + c, r, -k);
                t.frustum(r, -k, dk / 2.0, 0.0);
                t.socket(a, 0.0, -0.6 * k);
                t.flat(disc(r - c) + (disc(dk / 2.0) - hex_area(a)) + hex_area(a));
            }
            Kind::ButtonHead(_, l, dk, k, a) => {
                let d = major(&p);
                let (r, rk, flat) = (d / 2.0, dk / 2.0, 0.65 * a);
                let y0 = (k * k + flat * flat - rk * rk) / (2.0 * k);
                let rs = rk.hypot(y0);
                t.frustum(r, -l, r, 0.0);
                t.sphere_zone(y0, rs, 0.0, k);
                t.socket(a, k, 0.4 * k);
                t.flat(disc(r) + (disc(rk) - disc(r)) + (disc(flat) - hex_area(a)) + hex_area(a));
            }
            Kind::SetScrew(_, l, a, depth) => {
                let d = major(&p);
                let (r, c) = (d / 2.0, d / 10.0);
                t.frustum(r - c, 0.0, r, c);
                t.frustum(r, c, r, l - c);
                t.frustum(r, l - c, r - c, l);
                let depth = depth.min(0.8 * l);
                t.socket(a, l, l - depth);
                t.flat(disc(r - c) + (disc(r - c) - hex_area(a)) + hex_area(a));
            }
            Kind::FlangeBolt(_, l, a, k, dc) => {
                let d = major(&p);
                let (r, c, tf) = (d / 2.0, d / 10.0, 0.25 * k);
                t.frustum(r - c, -l, r, -l + c);
                t.frustum(r, -l + c, r, 0.0);
                t.frustum(dc / 2.0, 0.0, dc / 2.0, tf);
                t.hexagon(a, tf, k, 1.0);
                t.flat(
                    disc(r - c)
                        + (disc(dc / 2.0) - disc(r))
                        + (disc(dc / 2.0) - hex_area(a))
                        + hex_area(a),
                );
            }
            Kind::FlangeNut(_, a, m, dc) => {
                let r = minor_bore(&p) / 2.0;
                let tf = 0.25 * m;
                t.frustum(dc / 2.0, 0.0, dc / 2.0, tf);
                t.hexagon(a, tf, m, 1.0);
                t.bore(r, 0.0, m);
                t.flat(
                    (disc(dc / 2.0) - disc(r))
                        + (disc(dc / 2.0) - hex_area(a))
                        + (hex_area(a) - disc(r)),
                );
            }
            Kind::CapNut(_, a, m) => {
                let r = minor_bore(&p) / 2.0;
                let (hex, rd) = (0.5 * m, 0.45 * a);
                let rim = hex + 0.08 * m;
                let top = m - rim;
                let rs = (rd * rd + top * top) / (2.0 * top);
                t.hexagon(a, 0.0, hex, 1.0);
                t.frustum(rd, hex, rd, rim);
                t.sphere_zone(m - rs, rs, rim, m);
                t.bore(r, 0.0, 0.7 * m);
                t.flat((hex_area(a) - disc(r)) + (hex_area(a) - disc(rd)) + disc(r));
            }
            _ => continue,
        }
        check(&s, &p, &t);
    }
    assert!(seen > 20, "only {seen} members");
}

/// DIN 1587 M5's Join used to fail ("implausible result"); a domed nut has no
/// Boolean now.
#[test]
fn every_domed_nut_builds() {
    let mut n = 0;
    for s in all().iter().filter(|s| matches!(s.kind, Kind::CapNut(..))) {
        valid(
            &s.part()
                .unwrap_or_else(|e| panic!("{}: {e}", s.designation())),
        );
        n += 1;
    }
    assert!(n >= 5, "{n} domed nuts");
}

/// Rule 5: a chain that cannot close is refused and the part stays as it was.
#[test]
fn a_chain_that_cannot_close_is_refused() {
    use cadcraft3d::part::axial::{AxialOp, AxialRun};
    use cadcraft3d::part::{BodyOp, BodyOpFeature};
    let across = 10.0;
    let bad = [
        // Two runs that do not meet at one height.
        vec![
            AxialRun::turned(&[(2.0, -5.0, ""), (2.0, 0.0, "Shank")], Some("Seat")),
            AxialRun::hexagon(across, 1.0, 4.0, "Flat", Some("Top")),
        ],
        // A circle between the hexagon's flats and its corners.
        vec![
            AxialRun::turned(&[(5.4, -5.0, ""), (5.4, 0.0, "Shank")], Some("Seat")),
            AxialRun::hexagon(across, 0.0, 4.0, "Flat", Some("Top")),
        ],
    ];
    for runs in bad {
        let mut part = PartDocument::default();
        let r = part.add_body_op(BodyOpFeature::new(BodyOp::Axial(AxialOp {
            runs,
            first_cap: Some("End".to_owned()),
            closed: false,
        })));
        assert!(r.is_err());
        assert!(part.features.is_empty());
        assert_eq!(part.body.solids().count(), 0);
    }
}
