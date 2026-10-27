//! The owner's box (session 2026-10-27, `tests/data/kutia_session.3dcadcraft`):
//! a 200 × 80 × 20 box, rounded corners, split at z = 13 into a base and a
//! lid, both shelled 2 mm; a sketch on the lid's rim projects its whole
//! outline (straight and rounded edges, one closed loop each), the inner
//! loop is offset 1 mm into the wall and the ring between them is extruded
//! into a lip. Constant rounds give arcs; a Variable fillet's corners are
//! not circles and project as splines.

mod common;
use common::*;

use cadcraft3d::kernel::Real;
use cadcraft3d::part::{
    BlendFeature, ExtrudeFeature, Operation, OriginPlane, PartDocument, PlaneRef, ProfileRef,
    ShellFeature, SketchOptions, SketchPlacement, SplitFeature, SplitMethod, SplitToolRef, WorkDef,
    WorkFeature, WorkPlaneDef,
};

const W: Real = 200.0;
const D: Real = 80.0;
const H: Real = 20.0;
const CUT: Real = 13.0;
const R: Real = 10.0;

/// The box with its vertical edges rounded (r 10), split at z = 13 and
/// both halves shelled 2 mm from the cut.
fn box_split_shelled(variable: bool) -> PartDocument {
    let mut p = plate(W, D, H);
    let edges = vertical(&p, W, D, H);
    let mut f = BlendFeature::fillet(edges.clone(), R);
    if variable {
        // The owner's Variable fillet: r 10 at both ends of each edge.
        f.fillet_type = cadcraft3d::part::FilletType::Variable;
        f.variable = edges
            .into_iter()
            .map(|e| cadcraft3d::part::blend::VariableRadius {
                start: [e.hint[0], e.hint[1], 0.0],
                edge: e,
                points: vec![[0.0, R], [1.0, R]],
            })
            .collect();
        f.edges.clear();
    }
    p.add_blend(f).unwrap();
    let wp = p
        .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
            base: PlaneRef::Origin(OriginPlane::XY),
            distance: CUT,
        })))
        .unwrap();
    let mut s = SplitFeature::new(SplitMethod::SplitSolid);
    s.tool = Some(SplitToolRef::Plane(PlaneRef::Work(wp)));
    p.add_split(s).unwrap();
    for cut in ["Split1:Cut", "Split1#2:Cut"] {
        let mut sh = ShellFeature::new(2.0);
        sh.remove.push(face_ref(&p, cut, [W / 2.0, 1.0, CUT]));
        p.add_shell(sh)
            .unwrap_or_else(|e| panic!("shell {cut}: {e}"));
    }
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    p
}

/// The lid's rim: the face `Split1#2:Cut` piece left by the shell (a ring).
fn rim_sketch(p: &mut PartDocument) -> usize {
    let o = SketchOptions {
        auto_project_origin: false,
        ..Default::default()
    };
    p.create_sketch(SketchPlacement::on_face("Split1#2:Cut"), &o)
        .unwrap()
}

#[test]
fn the_rim_outline_projects_whole() {
    for variable in [false, true] {
        let mut p = box_split_shelled(variable);
        let j = rim_sketch(&mut p);
        let ps = &p.sketches[j];
        // 8 straight edges (4 outside, 4 inside) and 8 rounded corners.
        let curves = ps.sketch.entities().count();
        assert_eq!(curves, 16, "variable {variable}: {curves} projected curves");
        assert_eq!(ps.links.len(), 8, "the rounded corners are links");
        // Two regions: the ring (the inner loop is its hole) and the inside
        // of the inner loop.
        let regions = ps.sketch.regions();
        assert_eq!(
            regions.len(),
            2,
            "variable {variable}: {} regions",
            regions.len()
        );
        let ring = regions
            .iter()
            .find(|r| r.holes.len() == 1)
            .expect("the ring")
            .area();
        // Outside 200 × 80 with r 10 corners, inside 196 × 76 with r 8 (a
        // Variable fillet's corners only nearly).
        let round = |w: Real, d: Real, r: Real| w * d - (4.0 - std::f64::consts::PI) * r * r;
        let want = round(W, D, R) - round(W - 4.0, D - 4.0, R - 2.0);
        let tol = if variable { 0.02 } else { 1e-6 };
        assert!(
            (ring - want).abs() <= tol * want,
            "variable {variable}: ring {ring:.4} vs {want:.4}"
        );
        assert!(ps.health.is_none(), "{:?}", ps.health);
    }
}

#[test]
fn a_lip_from_the_offset_inner_wall() {
    lip(false);
}

#[test]
fn a_lip_round_variable_fillet_corners() {
    // The corners are splines: their offsets are splines too.
    lip(true);
}

/// The lip: the inner loop offset 1 mm into the wall, the ring between them
/// extruded 3 mm out of the rim and joined to the lid.
fn lip(variable: bool) {
    let mut p = box_split_shelled(variable);
    let before = volume(&p);
    let j = rim_sketch(&mut p);
    // The inner loop: a projected straight edge on the inside (y = 2).
    let sk = &p.sketches[j].sketch;
    let inner = sk
        .entities()
        .find(|(_, e)| match e {
            cadcraft3d::sketch::Entity::Line { start, end } => {
                let (a, b) = (
                    sk.point(*start).unwrap().position,
                    sk.point(*end).unwrap().position,
                );
                let (a, b) = (sk.plane.point_at(a.x, a.y), sk.plane.point_at(b.x, b.y));
                (a.y - 2.0).abs() < 1e-6 && (b.y - 2.0).abs() < 1e-6
            }
            _ => false,
        })
        .map(|(id, _)| id)
        .expect("the inner straight edge");
    // Offset the whole inner loop 1 mm into the wall (outwards).
    let ps = &mut p.sketches[j];
    let probe = {
        let e = ps.sketch.entity(inner).cloned().unwrap();
        let cadcraft3d::sketch::Entity::Line { start, end } = e else {
            panic!()
        };
        let (a, b) = (
            ps.sketch.point(start).unwrap().position,
            ps.sketch.point(end).unwrap().position,
        );
        let m = nalgebra::center(&a, &b);
        // A point just outside the inner loop (towards the outer wall).
        let world = ps.sketch.plane.point_at(m.x, m.y);
        ps.sketch.plane.project(&cadcraft3d::kernel::Point3::new(
            world.x,
            world.y - 1.0,
            world.z,
        ))
    };
    let d = ps.sketch.offset_side(inner, probe).unwrap();
    ps.sketch.offset(inner, d, true, true).unwrap();
    assert!(ps.sketch.solve().is_ok());
    // The ring between the inner wall and its offset: 1 mm wide.
    let regions = p.sketches[j].sketch.regions();
    let lip = regions
        .iter()
        .filter(|r| r.holes.len() == 1)
        .min_by(|a, b| a.area().total_cmp(&b.area()))
        .expect("regions")
        .clone();
    let ring = |w: Real, dd: Real, r: Real| w * dd - (4.0 - std::f64::consts::PI) * r * r;
    let want = ring(W - 2.0, D - 2.0, R - 1.0) - ring(W - 4.0, D - 4.0, R - 2.0);
    let tol = if variable { 0.02 } else { 1e-6 };
    assert!(
        (lip.area() - want).abs() < tol * want,
        "variable {variable}: lip area {} vs {want}",
        lip.area()
    );
    // Extruded 3 mm out of the rim and joined to the lid.
    let mut ex = ExtrudeFeature::new(vec![ProfileRef::of(&lip, lip.interior_point())], 3.0);
    ex.operation = Operation::Join;
    // Out of the rim, away from the lid (the sketch's normal is the rim's
    // outward normal, down; the default runs into the material).
    ex.direction = cadcraft3d::part::ExtentDirection::Flipped;
    let r = p.add_extrude(j, ex);
    r.unwrap_or_else(|e| panic!("lip: {e}"));
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    // The lip's volume is its own area × 3 (whatever the corners are).
    let added = volume(&p) - before;
    assert!(
        (added - lip.area() * 3.0).abs() < 1e-3 * want * 3.0,
        "variable {variable}: lip volume {added:.3} vs {:.3}",
        lip.area() * 3.0
    );
}

#[test]
fn the_owners_session_file_projects_its_corners() {
    // The saved session: its sketch on the lid had a corner refused ("not
    // circular any more": a Variable fillet's corners are no circles) and
    // the curved edges were not projected at all. Now every corner is
    // linked and the outline closes.
    let pr = cadcraft3d::document::Project::load(std::path::Path::new(
        "tests/data/kutia_session.3dcadcraft",
    ))
    .unwrap();
    let part = pr
        .documents()
        .iter()
        .find_map(|d| match &d.data {
            cadcraft3d::document::DocumentData::Part(p) => Some(p.clone()),
            _ => None,
        })
        .unwrap();
    let mut p = part;
    p.regenerate().unwrap();
    let face = p.sketches[1].placement.plane.clone();
    let PlaneRef::Face(face) = face else { panic!() };
    let o = SketchOptions {
        auto_project_origin: false,
        ..Default::default()
    };
    let j = p
        .create_sketch(SketchPlacement::on_face(&face), &o)
        .unwrap();
    let ps = &p.sketches[j];
    assert!(ps.health.is_none(), "{:?}", ps.health);
    assert_eq!(ps.links.len(), 4, "four rounded corners");
    let regions = ps.sketch.regions();
    // The lid of the saved file is not shelled (Shell2 was deleted): its
    // rim face is the whole rounded outline, one closed loop.
    assert_eq!(regions.len(), 1, "the outline is closed");
    assert!(regions[0].holes.is_empty());
}

#[test]
fn rounds_cut_by_a_split_project_as_arcs() {
    // The owner's box (2026-10-27): Variable fillet corners r 10, top and
    // bottom edges rounded, split at z = 13, a lip on the lid. The kernel's
    // round is a spline fitted to the cylinder, so the section edge on the
    // lid's cut face was a spline 0.01 mm off the circle and projected as a
    // spline (its handles could be dragged, its offset did not follow a
    // dimension). It projects as an arc of the round's cylinder now.
    let pr = cadcraft3d::document::Project::load(std::path::Path::new(
        "tests/data/kutia_lid_lip.3dcadcraft",
    ))
    .unwrap();
    let mut p = pr
        .documents()
        .iter()
        .find_map(|d| match &d.data {
            cadcraft3d::document::DocumentData::Part(p) => Some(p.clone()),
            _ => None,
        })
        .unwrap();
    p.regenerate().unwrap();
    let o = SketchOptions {
        auto_project_origin: false,
        ..Default::default()
    };
    let j = p
        .create_sketch(SketchPlacement::on_face("Split1#2:Cut"), &o)
        .unwrap();
    let ps = &p.sketches[j];
    assert!(ps.health.is_none(), "{:?}", ps.health);
    assert_eq!(ps.links.len(), 4, "four rounded corners");
    for l in &ps.links {
        let c = l.curve.expect("a curve");
        let Some(cadcraft3d::sketch::Entity::Arc { center, start, end }) = ps.sketch.entity(c)
        else {
            panic!("{:?} is not an arc", ps.sketch.entity(c));
        };
        let at = |q| ps.sketch.point(q).unwrap().position;
        let (r0, r1) = (
            (at(*start) - at(*center)).norm(),
            (at(*end) - at(*center)).norm(),
        );
        assert!(
            (r0 - R).abs() < 1e-2 && (r0 - r1).abs() < 1e-9,
            "radius {r0} {r1}"
        );
    }
    // The lip (Extrusion2) stands on the face: the outline and the lip's
    // inner loop give the ring and its inside, together the whole face.
    let regions = ps.sketch.regions();
    assert_eq!(regions.len(), 2);
    let want = W * D - (4.0 - std::f64::consts::PI) * R * R;
    let got: Real = regions.iter().map(|r| r.area()).sum();
    assert!((got - want).abs() < 1e-5 * want, "area {got} vs {want}");
}

#[test]
fn a_lip_flush_with_the_lids_outline_joins_exactly() {
    // The owner's box: the lid's cut face outline projected (arcs at the
    // rounded corners), offset 3 mm inwards and the ring extruded 3 mm out
    // of the face, joined to the lid. The kernel's Union of that tool (its
    // outer wall flush with the lid's walls) found no result after a minute
    // and the whole part fell back to the faceted kernel (frames of 60-80
    // ms after it). Now the face is thickened into a slab and the inside cut
    // back out: exact, well under a second.
    let pr = cadcraft3d::document::Project::load(std::path::Path::new(
        "tests/data/kutia_lid_lip.3dcadcraft",
    ))
    .unwrap();
    let mut p = pr
        .documents()
        .iter()
        .find_map(|d| match &d.data {
            cadcraft3d::document::DocumentData::Part(p) => Some(p.clone()),
            _ => None,
        })
        .unwrap();
    p.regenerate().unwrap();
    // Without the saved lip (its sketch had splines at the corners).
    let k = p.features.len() - 1;
    p.remove_feature(k).unwrap();
    let j = p.sketches.iter().position(|s| s.id == "Sketch2").unwrap();
    p.delete_sketch(j).unwrap();
    p.regenerate().unwrap();
    let lid_volume = |p: &PartDocument| {
        p.exact_bodies
            .0
            .iter()
            .find(|s| s.name.as_deref() == Some("Split1#2"))
            .and_then(|s| s.volume_at(1e-4))
            .unwrap()
    };
    let v0 = lid_volume(&p);
    let o = SketchOptions {
        auto_project_origin: false,
        ..Default::default()
    };
    let j = p
        .create_sketch(SketchPlacement::on_face("Split1#2:Cut"), &o)
        .unwrap();
    let line = p.sketches[j]
        .sketch
        .entities()
        .find(|(_, e)| e.is_line())
        .map(|(id, _)| id)
        .unwrap();
    let sk = &mut p.sketches[j].sketch;
    // Inwards: towards the middle of the face (sketch y runs along −y).
    let side = sk
        .offset_side(line, cadcraft3d::sketch::Point2::new(W / 2.0, -D / 2.0))
        .unwrap();
    sk.offset(line, side.signum() * 3.0, true, true).unwrap();
    sk.solve().unwrap();
    let regions = p.sketches[j].sketch.regions();
    let ring = regions.iter().find(|r| r.holes.len() == 1).unwrap();
    let area = ring.area();
    let mut f = ExtrudeFeature::new(vec![ProfileRef::of(ring, ring.interior_point())], 3.0);
    f.operation = Operation::Join;
    f.solids = vec!["Split1#2".into()];
    let t = std::time::Instant::now();
    let id = p.add_extrude(j, f).unwrap();
    let took = t.elapsed();
    assert!(p.exact, "exact kernel");
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    assert!(took.as_secs_f64() < 5.0, "{took:?}");
    let added = lid_volume(&p) - v0;
    assert!(
        (added - 3.0 * area).abs() < 2e-4 * 3.0 * area,
        "added {added} vs {}",
        3.0 * area
    );
    let names: Vec<String> = p.body.faces().filter_map(|(_, f)| f.name.clone()).collect();
    assert!(names.contains(&format!("{id}:End")), "{names:?}");
}
