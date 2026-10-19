//! Helpers shared by the cross-feature tests (`combos.rs`,
//! `freecad_cases.rs`).
#![allow(dead_code, unused_imports)]

pub use std::f64::consts::PI;

pub use cadcraft3d::document::{DocumentData, Project};
pub use cadcraft3d::kernel::{Point3, Real, TessellationOptions};
pub use cadcraft3d::part::blend::edge_ref;
pub use cadcraft3d::part::{
    AxisRef, BlendFeature, DraftFeature, DraftType, EdgeRef, Extent, ExtentDirection,
    ExtrudeFeature, FaceRef, Feature, Operation, OriginPlane, PartDocument, PlaneRef, ProfileRef,
    RevolveFeature, ShellDirection, ShellFeature, SketchOptions, SketchPlacement,
};
pub use cadcraft3d::sketch::{Point2, Region};

// ------------------------------------------------------------ helpers ---

pub fn opts() -> SketchOptions {
    SketchOptions {
        auto_project_origin: false,
        ..Default::default()
    }
}

/// Volume of all solids: the exact kernel's measure (fine tessellation),
/// else our display triangles.
pub fn volume(part: &PartDocument) -> Real {
    if part.exact && !part.exact_bodies.0.is_empty() {
        return part
            .exact_bodies
            .0
            .iter()
            .map(|b| b.measure_full().map(|m| m.volume).unwrap_or(0.0))
            .sum();
    }
    let o = TessellationOptions::default();
    part.body
        .solids()
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

/// Exact kernel, no sick feature, valid solids, volume within `rel`.
pub fn check(part: &PartDocument, expect: Real, rel: Real) {
    assert!(part.exact, "built with the faceted kernel");
    assert!(
        part.feature_health.is_empty(),
        "sick: {:?}",
        part.feature_health
    );
    for (sid, _) in part.body.solids() {
        part.body
            .validate_solid(sid)
            .unwrap_or_else(|e| panic!("invalid solid: {e:?}"));
    }
    let v = volume(part);
    assert!(
        (v - expect).abs() <= rel * expect.abs().max(1.0),
        "volume {v} vs {expect}"
    );
}

/// A refusal containing `text`; the part keeps its features.
pub fn refused<T: std::fmt::Debug>(r: Result<T, impl std::fmt::Display>, text: &str) {
    match r {
        Ok(v) => panic!("built ({v:?}), expected a refusal with '{text}'"),
        Err(e) => {
            let e = e.to_string();
            assert!(e.contains(text), "'{e}' does not contain '{text}'");
        }
    }
}

pub fn region_profiles(regions: &[Region], pick: impl Fn(&Region) -> bool) -> Vec<ProfileRef> {
    regions
        .iter()
        .filter(|r| pick(r))
        .map(|r| ProfileRef::of(r, r.interior_point()))
        .collect()
}

/// A W×D rectangle at the origin extruded H.
pub fn plate(w: Real, d: Real, h: Real) -> PartDocument {
    let mut part = PartDocument::default();
    let i = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    let sk = &mut part.sketches[i].sketch;
    sk.add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(w, d))
        .unwrap();
    let r = sk.regions();
    part.add_extrude(i, ExtrudeFeature::new(region_profiles(&r, |_| true), h))
        .unwrap();
    part
}

/// Extrude regions of sketch `j` joined to the body (the feature's default
/// is New Solid, for old files).
pub fn boss(part: &mut PartDocument, j: usize, h: Real, pick: impl Fn(&Region) -> bool) {
    let r = part.sketches[j].sketch.regions();
    let mut ex = ExtrudeFeature::new(region_profiles(&r, pick), h);
    ex.operation = Operation::Join;
    part.add_extrude(j, ex).unwrap();
}

/// A sketch on face `face` (at height z, or any plane) drawn in model x/y.
pub fn sketch_on(
    part: &mut PartDocument,
    face: &str,
    draw: impl FnOnce(&mut cadcraft3d::sketch::Sketch, &dyn Fn(Real, Real, Real) -> Point2),
) -> usize {
    let j = part
        .create_sketch(SketchPlacement::on_face(face), &opts())
        .unwrap();
    let plane = part.sketches[j].sketch.plane.clone();
    let at = move |x: Real, y: Real, z: Real| plane.project(&Point3::new(x, y, z));
    draw(&mut part.sketches[j].sketch, &at);
    j
}

/// The name of the face all of whose display points satisfy `on`.
pub fn face_where(part: &PartDocument, on: impl Fn(&Point3) -> bool) -> String {
    part.body
        .faces()
        .find(|(_, f)| f.mesh.as_ref().is_some_and(|m| m.positions.iter().all(&on)))
        .and_then(|(_, f)| f.name.clone())
        .expect("no such face")
}

pub fn face_ref(part: &PartDocument, name: &str, at: [Real; 3]) -> FaceRef {
    part.body
        .faces()
        .filter(|(_, f)| f.name.as_deref() == Some(name))
        .find_map(|(fid, _)| {
            cadcraft3d::part::shell::face_ref(&part.body, fid, Some(Point3::from(at)))
        })
        .expect("no face")
}

pub fn edges_where(part: &PartDocument, keep: impl Fn([Real; 3]) -> bool) -> Vec<EdgeRef> {
    part.body
        .edges()
        .filter_map(|(e, _)| edge_ref(&part.body, e))
        .filter(|r| keep(r.hint))
        .collect()
}

/// Cut the regions `pick` chooses from sketch `j`: through all, or `depth`.
pub fn cut(part: &mut PartDocument, j: usize, depth: Option<Real>, pick: impl Fn(&Region) -> bool) {
    let r = part.sketches[j].sketch.regions();
    let mut ex = ExtrudeFeature::new(region_profiles(&r, pick), depth.unwrap_or(1.0));
    ex.operation = Operation::Cut;
    ex.direction = ExtentDirection::Flipped;
    if depth.is_none() {
        ex.extent = Extent::ThroughAll;
    }
    part.add_extrude(j, ex).unwrap();
}

pub fn shell(part: &mut PartDocument, faces: &[(&str, [Real; 3])], t: Real, dir: ShellDirection) {
    let mut def = ShellFeature::new(t);
    def.direction = dir;
    for (n, at) in faces {
        def.remove.push(face_ref(part, n, *at));
    }
    part.add_shell(def).unwrap();
}

pub fn top(part: &PartDocument, z: Real) -> String {
    face_where(part, |p| (p.z - z).abs() < 1e-6)
}

pub fn k(deg: Real) -> Real {
    deg.to_radians().tan()
}

pub fn simpson(f: &dyn Fn(Real) -> Real, a: Real, b: Real) -> Real {
    (b - a) / 6.0 * (f(a) + 4.0 * f((a + b) / 2.0) + f(b))
}

/// Four side faces of the W×D plate at height z.
pub fn sides(part: &PartDocument, w: Real, d: Real, z: Real) -> Vec<(String, [Real; 3])> {
    vec![
        (face_where(part, |p| p.y.abs() < 1e-6), [w / 2.0, 0.0, z]),
        (
            face_where(part, |p| (p.y - d).abs() < 1e-6),
            [w / 2.0, d, z],
        ),
        (face_where(part, |p| p.x.abs() < 1e-6), [0.0, d / 2.0, z]),
        (
            face_where(part, |p| (p.x - w).abs() < 1e-6),
            [w, d / 2.0, z],
        ),
    ]
}

pub fn draft_sides(part: &mut PartDocument, faces: &[(String, [Real; 3])], deg: Real) {
    let mut d = DraftFeature::new(deg.to_radians());
    d.draft_type = DraftType::FixedPlane;
    d.plane = Some(PlaneRef::Origin(OriginPlane::XY));
    for (n, at) in faces {
        d.faces.push(face_ref(part, n, *at));
    }
    part.add_draft(d).unwrap();
}

/// The 4 vertical edges of a W×D×H plate (every piece).
pub fn vertical(part: &PartDocument, w: Real, d: Real, h: Real) -> Vec<EdgeRef> {
    let c = |x: Real, m: Real| x.abs() < 1e-6 || (x - m).abs() < 1e-6;
    edges_where(part, |p| {
        c(p[0], w) && c(p[1], d) && p[2] > 1e-6 && p[2] < h - 1e-6
    })
}

/// Lost area of a round r at a 90° edge.
pub fn round_loss(r: Real) -> Real {
    (1.0 - PI / 4.0) * r * r
}
