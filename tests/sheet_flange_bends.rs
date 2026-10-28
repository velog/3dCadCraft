//! Flange › Further Bends (`docs/spec/5.6-sheet-metal.md` §3.4, SolidCraft's `then`): one
//! Flange feature with a chain of bends, each a flange on the far edge of the one before. Every
//! variant is checked against the same part built from separate Flange features on those far
//! edges: the same volume, box and flat pattern.

use cadcraft3d::kernel::{Point3, Real};
use cadcraft3d::part::Feature;
use cadcraft3d::part::sheet_metal::{
    FlangeBend, FlangeWidth, SheetEdge, SheetFlangeFeature, SheetMetalDefaults,
};
use cadcraft3d::part::{OriginPlane, PartDocument, SketchOptions, SketchPlacement};
use cadcraft3d::sketch::Point2;

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4};

/// A 100 × 60 sheet plate on XY (the rule's thickness, 2 mm).
fn plate() -> PartDocument {
    let mut p = PartDocument::default();
    p.make_sheet_metal(Some(SheetMetalDefaults::default()));
    let o = SketchOptions {
        auto_project_origin: false,
        ..Default::default()
    };
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &o)
        .unwrap();
    p.sketches[i]
        .sketch
        .add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(100.0, 60.0))
        .unwrap();
    let regions = p.sketches[i].sketch.regions();
    let r = &regions[0];
    let profile = cadcraft3d::part::ProfileRef::of(r, r.interior_point());
    p.add_sheet_face(i, vec![profile], false).unwrap();
    p
}

fn volume(p: &PartDocument) -> Real {
    p.exact_bodies
        .0
        .iter()
        .filter_map(|s| s.volume_at(1e-3))
        .sum()
}

fn check(p: &PartDocument) {
    assert!(p.exact, "exact kernel");
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    for (sid, _) in p.body.solids() {
        p.body.validate_solid(sid).unwrap();
    }
    assert!(p.body.faces().all(|(_, f)| f.name.is_some()), "named");
}

fn edges_at(p: &PartDocument, at: &[Point3]) -> (String, Vec<SheetEdge>) {
    let mut body = String::new();
    let mut edges = Vec::new();
    for &a in at {
        let (b, e) = p
            .sheet_edge_at(a)
            .unwrap_or_else(|| panic!("no sheet edge at {a}"));
        body = b;
        edges.push(e);
    }
    (body, edges)
}

/// The base edges of the plate's top face: front (y = 0), right, back, left.
fn base_points(n: usize) -> Vec<Point3> {
    [
        Point3::new(50.0, 0.0, 2.0),
        Point3::new(100.0, 30.0, 2.0),
        Point3::new(50.0, 60.0, 2.0),
        Point3::new(0.0, 30.0, 2.0),
    ][..n]
        .to_vec()
}

/// One Flange with further bends, and the same part made of one Flange
/// per bend (each on the far edges of the one before, on the same face as
/// the edge before unless flipped).
fn both(
    n: usize,
    first: impl Fn(&mut SheetFlangeFeature),
    bends: &[FlangeBend],
) -> (PartDocument, PartDocument) {
    let mut one = plate();
    let (body, edges) = edges_at(&one, &base_points(n));
    let mut def = SheetFlangeFeature::new(&body, edges.clone(), 20.0);
    first(&mut def);
    def.bends = bends.to_vec();
    one.add_sheet_flange(def.clone()).unwrap();

    let mut many = plate();
    let mut f0 = def.clone();
    f0.bends.clear();
    let id0 = many.add_sheet_flange(f0).unwrap();
    let mut prev = id0;
    let mut top: Vec<bool> = edges.iter().map(|e| e.name.ends_with(":top")).collect();
    for b in bends {
        for t in &mut top {
            *t ^= b.flip;
        }
        let far: Vec<SheetEdge> = (1..=n)
            .map(|k| {
                let side = if top[k - 1] { "top" } else { "bottom" };
                let name = format!("{prev}.{k}:far:{side}");
                let (_, e) = edges_at(&many, &[far_point(&many, &body, &name)]);
                assert_eq!(e[0].name, name, "the far edge by its point");
                e[0].clone()
            })
            .collect();
        let mut f = SheetFlangeFeature::new(&body, far, b.height);
        f.angle = b.angle;
        f.position = def.position;
        f.radius = def.radius;
        prev = many.add_sheet_flange(f).unwrap();
    }
    (one, many)
}

/// A point on the sheet edge `name` of `body`.
fn far_point(p: &PartDocument, body: &str, name: &str) -> Point3 {
    let e = SheetEdge {
        name: name.to_owned(),
        hint: [50.0, 30.0, 0.0],
    };
    let (a, b) = p
        .sheet_edge_ends(body, &e)
        .unwrap_or_else(|| panic!("no edge {name}"));
    Point3::from((a.coords + b.coords) * 0.5)
}

fn same(one: &PartDocument, many: &PartDocument) {
    check(one);
    check(many);
    let (v1, v2) = (volume(one), volume(many));
    assert!((v1 - v2).abs() < 1e-6 * v2.max(1.0), "volume {v1} vs {v2}");
    let (a1, b1) = one.body.bounding_box().unwrap();
    let (a2, b2) = many.body.bounding_box().unwrap();
    assert!((a1 - a2).norm() < 1e-3 && (b1 - b2).norm() < 1e-3, "box");
    let f1 = &one.flat_patterns()[0];
    let f2 = &many.flat_patterns()[0];
    assert_eq!(f1.bends.len(), f2.bends.len(), "bends");
    let (s1, s2) = (one.flat_extents().unwrap(), many.flat_extents().unwrap());
    assert!(
        (s1.length - s2.length).abs() < 1e-6 && (s1.width - s2.width).abs() < 1e-6,
        "flat {s1:?} vs {s2:?}"
    );
}

fn bend(height: Real, angle: Real, flip: bool) -> FlangeBend {
    FlangeBend {
        height,
        angle,
        flip,
    }
}

#[test]
fn one_further_bend_either_way() {
    for flip in [false, true] {
        let (one, many) = both(1, |_| {}, &[bend(15.0, FRAC_PI_2, flip)]);
        same(&one, &many);
        assert_eq!(one.features.len(), 2, "plate + one Flange");
        assert_eq!(one.flat_patterns()[0].bends.len(), 2);
        // The second bend leaves the wall's top at z = 20: Default turns the
        // same way as the first (back over the plate, a U), Flipped the other
        // way (away from it, a Z).
        let (lo, hi) = one.body.bounding_box().unwrap();
        assert!((hi.z - 20.0).abs() < 0.1, "wall top {hi}");
        if flip {
            assert!(lo.y < -12.0, "flipped: away from the plate {lo} {hi}");
        } else {
            assert!(lo.y > -0.1, "default: over the plate {lo} {hi}");
        }
    }
}

#[test]
fn a_chain_of_angles() {
    let chain = [
        bend(15.0, FRAC_PI_2, false),
        bend(10.0, FRAC_PI_4, true),
        bend(8.0, 2.0, false),
    ];
    let (one, many) = both(1, |_| {}, &chain);
    same(&one, &many);
    assert_eq!(one.flat_patterns()[0].bends.len(), 4);
}

#[test]
fn further_bends_on_every_edge_close_their_corners() {
    let (one, many) = both(4, |_| {}, &[bend(12.0, FRAC_PI_2, false)]);
    same(&one, &many);
    assert_eq!(one.flat_patterns()[0].bends.len(), 8);
}

#[test]
fn further_bends_after_a_narrower_first_bend() {
    let (one, many) = both(
        1,
        |f| {
            f.extents = FlangeWidth::Offset;
            (f.offset1, f.offset2) = (10.0, 20.0);
            f.angle = 1.2;
        },
        &[bend(10.0, FRAC_PI_2, true), bend(6.0, FRAC_PI_2, true)],
    );
    same(&one, &many);
}

#[test]
fn bend_fields_are_parameters_and_survive_a_save() {
    let mut p = plate();
    let (body, edges) = edges_at(&p, &base_points(1));
    let mut def = SheetFlangeFeature::new(&body, edges, 20.0);
    def.bends = vec![bend(15.0, FRAC_PI_2, false), bend(5.0, FRAC_PI_2, true)];
    let id = p.add_sheet_flange(def).unwrap();
    let k = p.features.iter().position(|f| f.id() == id).unwrap();
    let Some(Feature::SheetFlange(f)) = p.features.get(k) else {
        panic!("a flange")
    };
    let fields: Vec<&str> = f.fields().iter().map(|(n, _)| *n).collect();
    for want in ["height2", "angle2", "height3", "angle3"] {
        assert!(fields.contains(&want), "{fields:?}");
    }
    assert_eq!(f.value("height3"), Some(5.0));
    // Each bend's fields are named model parameters, like the first's.
    for field in ["height", "height2", "angle3"] {
        assert!(
            f.params.0.get(field).is_some_and(|p| !p.name.is_empty()),
            "{field}: {:?}",
            f.params
        );
    }
    // Through the part's parameters: height2 = the first height.
    let v0 = volume(&p);
    let mut q = p.clone();
    let mut f2 = f.clone();
    f2.set("height2", 20.0);
    q.replace_feature(k, Feature::SheetFlange(f2)).unwrap();
    check(&q);
    assert!(volume(&q) > v0, "a longer second bend");
    // Remove the first further bend: the second moves up, its value too.
    let mut f3 = f.clone();
    f3.remove_bend(0);
    assert_eq!(f3.bends.len(), 1);
    assert_eq!(f3.value("height2"), Some(5.0));
    assert_eq!(f3.value("height3"), None);
    assert!(f3.add_bend());
    assert_eq!(f3.value("height3"), Some(5.0), "a new bend copies the last");
    // Save and load.
    let text = serde_json::to_string(&p.features[k]).unwrap();
    assert!(text.contains("\"bends\""), "{text}");
    let back: Feature = serde_json::from_str(&text).unwrap();
    assert_eq!(back, p.features[k]);
    // A flange without further bends writes nothing new (old builds read it).
    let plain = serde_json::to_string(&Feature::SheetFlange(SheetFlangeFeature::new(
        &body,
        Vec::new(),
        10.0,
    )))
    .unwrap();
    assert!(!plain.contains("bends"), "{plain}");
}

#[test]
fn a_bad_further_bend_leaves_the_part_unchanged() {
    let mut p = plate();
    let (body, edges) = edges_at(&p, &base_points(1));
    let mut def = SheetFlangeFeature::new(&body, edges, 20.0);
    def.bends = vec![bend(0.0, FRAC_PI_2, false)];
    let before = p.features.len();
    let v = volume(&p);
    let err = p.add_sheet_flange(def).unwrap_err().to_string();
    assert!(err.contains("bend 2"), "{err}");
    assert_eq!(p.features.len(), before);
    assert!((volume(&p) - v).abs() < 1e-9);
}

#[test]
fn further_bends_of_a_flange_on_the_bottom_face_turn_alike() {
    // The flange on the plate's bottom edge goes down; Default turns back
    // under the plate (a U), Flipped away from it (a Z) — as on the top.
    for flip in [false, true] {
        let mut one = plate();
        let (body, edges) = edges_at(&one, &[Point3::new(50.0, 0.0, 0.0)]);
        assert!(edges[0].name.ends_with(":bottom"), "{}", edges[0].name);
        let mut def = SheetFlangeFeature::new(&body, edges, 20.0);
        def.bends = vec![bend(15.0, FRAC_PI_2, flip)];
        one.add_sheet_flange(def).unwrap();
        check(&one);
        let (lo, hi) = one.body.bounding_box().unwrap();
        assert!(lo.z < -15.0 && hi.z < 2.1, "downwards {lo} {hi}");
        if flip {
            assert!(lo.y < -12.0, "flipped: away from the plate {lo} {hi}");
        } else {
            assert!(lo.y > -0.1, "default: under the plate {lo} {hi}");
        }
    }
}
