//! The basic features on simple, complex and very complex geometry
//! (`docs/spec/TESTING.md`): each case must build with the exact kernel (or
//! fail with a clear message where that is the right answer), give a valid
//! B-rep, match its analytic volume and stay within its time budget.
//! Run with `--nocapture` for the table.

use std::f64::consts::PI;
use std::time::Instant;

use cadcraft3d::kernel::{Point3, Real};
use cadcraft3d::part::PartialChamfer;
use cadcraft3d::part::blend::edge_ref;
use cadcraft3d::part::{
    BlendFeature, EdgeRef, Extent, ExtentDirection, ExtrudeFeature, FaceRef, Feature, FilletSet,
    Operation, OriginPlane, PartDocument, ProfileRef, RevolveFeature, ShellDirection, ShellFeature,
    SketchOptions, SketchPlacement, VariableRadius,
};
use cadcraft3d::part::{DraftFeature, DraftType, PlaneRef, PullRef};
use cadcraft3d::part::{DrillPoint, HoleCenter, HoleFeature, HoleSpec, SeatType, Termination};
use cadcraft3d::sketch::{Entity, Point2, Region};

/// What a case expects.
enum Want {
    /// Built exactly, with this volume (mm³).
    Volume(Real),
    /// Built exactly, with this volume within an absolute tolerance (mm³):
    /// for small blends on a large part, where 2e-3 of the total would hide
    /// a missing round (the tessellated volume itself is ~3 mm³ off here).
    VolumeWithin(Real, Real),
    /// Refused with a message containing this text; the part unchanged.
    Error(&'static str),
}

struct Case {
    level: &'static str,
    name: &'static str,
    budget_ms: Real,
    build: fn() -> Result<PartDocument, String>,
    want: Want,
}

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

fn no_projection() -> SketchOptions {
    SketchOptions {
        auto_project_origin: false,
        ..Default::default()
    }
}

/// A rectangle W×D extruded H on XY.
fn plate(w: Real, d: Real, h: Real) -> Result<PartDocument, String> {
    let mut part = PartDocument::default();
    let i = part
        .create_sketch(
            SketchPlacement::on_origin_plane(OriginPlane::XY),
            &no_projection(),
        )
        .map_err(|e| e.to_string())?;
    let sk = &mut part.sketches[i].sketch;
    sk.add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(w, d))
        .map_err(|e| e.to_string())?;
    let r = sk.regions();
    part.add_extrude(
        i,
        ExtrudeFeature::new(vec![ProfileRef::of(&r[0], r[0].interior_point())], h),
    )
    .map_err(|e| e.to_string())?;
    Ok(part)
}

/// A sketch on the top face (`face`) of `part`; `draw` adds geometry in model
/// x/y (the face is at height z), returns the sketch index.
fn sketch_on_top(
    part: &mut PartDocument,
    face: &str,
    z: Real,
    draw: impl FnOnce(
        &mut cadcraft3d::sketch::Sketch,
        &dyn Fn(Real, Real) -> Point2,
    ) -> Result<(), String>,
) -> Result<usize, String> {
    let j = part
        .create_sketch(SketchPlacement::on_face(face), &no_projection())
        .map_err(|e| e.to_string())?;
    let plane = part.sketches[j].sketch.plane.clone();
    let at = move |x: Real, y: Real| plane.project(&Point3::new(x, y, z));
    draw(&mut part.sketches[j].sketch, &at)?;
    Ok(j)
}

/// Cut through all, against the sketch normal, the regions `pick` chooses.
fn cut_regions(
    part: &mut PartDocument,
    j: usize,
    pick: impl Fn(&Region) -> bool,
) -> Result<(), String> {
    let regions = part.sketches[j].sketch.regions();
    let profiles: Vec<ProfileRef> = regions
        .iter()
        .filter(|r| pick(r))
        .map(|r| ProfileRef::of(r, r.interior_point()))
        .collect();
    if profiles.is_empty() {
        return Err("no region picked".into());
    }
    let mut ex = ExtrudeFeature::new(profiles, 1.0);
    ex.operation = Operation::Cut;
    ex.extent = Extent::ThroughAll;
    ex.direction = ExtentDirection::Flipped;
    part.add_extrude(j, ex).map_err(|e| e.to_string())?;
    Ok(())
}

fn edges_where(part: &PartDocument, keep: impl Fn([Real; 3]) -> bool) -> Vec<EdgeRef> {
    part.body
        .edges()
        .filter_map(|(e, _)| edge_ref(&part.body, e))
        .filter(|r| keep(r.hint))
        .collect()
}

// ------------------------------------------------------------- simple ---

fn s_box() -> Result<PartDocument, String> {
    plate(40.0, 30.0, 20.0)
}

fn s_cylinder() -> Result<PartDocument, String> {
    let mut part = PartDocument::default();
    let i = part
        .create_sketch(
            SketchPlacement::on_origin_plane(OriginPlane::XY),
            &no_projection(),
        )
        .map_err(|e| e.to_string())?;
    part.sketches[i]
        .sketch
        .add_circle(Point2::new(0.0, 0.0), 15.0)
        .map_err(|e| e.to_string())?;
    let r = part.sketches[i].sketch.regions();
    part.add_extrude(
        i,
        ExtrudeFeature::new(vec![ProfileRef::of(&r[0], r[0].interior_point())], 50.0),
    )
    .map_err(|e| e.to_string())?;
    Ok(part)
}

/// A 10×20 rectangle 10 off the axis revolved 360°: a tube.
fn s_revolve_tube() -> Result<PartDocument, String> {
    let mut part = PartDocument::default();
    let i = part
        .create_sketch(
            SketchPlacement::on_origin_plane(OriginPlane::XZ),
            &no_projection(),
        )
        .map_err(|e| e.to_string())?;
    let sk = &mut part.sketches[i].sketch;
    sk.add_rectangle_two_point(Point2::new(10.0, 0.0), Point2::new(20.0, 20.0))
        .map_err(|e| e.to_string())?;
    let r = sk.regions();
    let def = RevolveFeature::new(
        vec![ProfileRef::of(&r[0], r[0].interior_point())],
        cadcraft3d::part::AxisRef::Origin(2),
    );
    part.add_revolve(i, def).map_err(|e| e.to_string())?;
    Ok(part)
}

fn s_fillet_edge() -> Result<PartDocument, String> {
    let mut part = plate(40.0, 30.0, 20.0)?;
    let e = edges_where(&part, |h| (h[0] - 40.0).abs() < 1e-6 && h[1].abs() < 1e-6);
    part.add_blend(BlendFeature::fillet(e, 5.0))
        .map_err(|e| e.to_string())?;
    Ok(part)
}

fn s_through_hole() -> Result<PartDocument, String> {
    let mut part = plate(40.0, 30.0, 10.0)?;
    let j = sketch_on_top(&mut part, "Extrusion1:End", 10.0, |sk, at| {
        sk.add_circle(at(20.0, 15.0), 5.0)
            .map(|_| ())
            .map_err(|e| e.to_string())
    })?;
    cut_regions(&mut part, j, |r| r.holes.is_empty() && r.area() < 100.0)?;
    Ok(part)
}

// ------------------------------------------------------------ complex ---

/// Disc r 20 × 5; on top a circle r 15 and a radial line patterned 8
/// times (circular sketch pattern through the centre); every other sector
/// cut through: a radial grille.
fn c_radial_grille() -> Result<PartDocument, String> {
    radial_grille(8, 20.0, 15.0)
}

fn radial_grille(n: usize, big: Real, small: Real) -> Result<PartDocument, String> {
    let mut part = PartDocument::default();
    let i = part
        .create_sketch(
            SketchPlacement::on_origin_plane(OriginPlane::XY),
            &no_projection(),
        )
        .map_err(|e| e.to_string())?;
    part.sketches[i]
        .sketch
        .add_circle(Point2::new(0.0, 0.0), big)
        .map_err(|e| e.to_string())?;
    let r = part.sketches[i].sketch.regions();
    part.add_extrude(
        i,
        ExtrudeFeature::new(vec![ProfileRef::of(&r[0], r[0].interior_point())], 5.0),
    )
    .map_err(|e| e.to_string())?;
    let j = sketch_on_top(&mut part, "Extrusion1:End", 5.0, |sk, at| {
        let c = sk
            .add_circle(at(0.0, 0.0), small)
            .map_err(|e| e.to_string())?;
        let Some(Entity::Circle { center, .. }) = sk.entity(c).cloned() else {
            return Err("circle".into());
        };
        let b = sk.add_point(at(small + 3.0, 0.0));
        let l = sk.add_line_between(center, b).map_err(|e| e.to_string())?;
        sk.circular_pattern(&[l], center, n, std::f64::consts::TAU, true)
            .map_err(|e| e.to_string())?;
        sk.solve().map_err(|e| e.to_string())?;
        Ok(())
    })?;
    let mut sectors: Vec<Region> = part.sketches[j]
        .sketch
        .regions()
        .into_iter()
        .filter(|r| r.area() < PI * small * small / n as Real * 1.01)
        .collect();
    if sectors.len() != n {
        return Err(format!("{} sectors instead of {n}", sectors.len()));
    }
    sectors.sort_by(|a, b| {
        let ang = |r: &Region| {
            let p = r.interior_point();
            p.y.atan2(p.x)
        };
        ang(a).total_cmp(&ang(b))
    });
    let keep: Vec<Point2> = sectors
        .iter()
        .step_by(2)
        .map(|r| r.interior_point())
        .collect();
    cut_regions(&mut part, j, |r| {
        keep.iter().any(|p| r.contains(*p)) && r.area() < PI * small * small / n as Real * 1.01
    })?;
    Ok(part)
}

/// Checkerboard ventilation grille: a 60×60×3 plate, a grid of 6×6 cells
/// of 10 mm, a 8×8 hole in every other cell (a 2 mm web between them).
fn c_checker_grille() -> Result<PartDocument, String> {
    checker_grille(6, 10.0, 8.0)
}

fn checker_grille(n: usize, cell: Real, hole: Real) -> Result<PartDocument, String> {
    let side = n as Real * cell;
    let mut part = plate(side, side, 3.0)?;
    let m = (cell - hole) / 2.0;
    let j = sketch_on_top(&mut part, "Extrusion1:End", 3.0, |sk, at| {
        for i in 0..n {
            for k in 0..n {
                if (i + k) % 2 == 0 {
                    let (x, y) = (i as Real * cell + m, k as Real * cell + m);
                    sk.add_rectangle_two_point(at(x, y), at(x + hole, y + hole))
                        .map_err(|e| e.to_string())?;
                }
            }
        }
        Ok(())
    })?;
    // The holes by their centres (where they touch at corners, the cells
    // between them are regions of the same size too).
    let plane = part.sketches[j].sketch.plane.clone();
    let centres: Vec<Point2> = (0..n)
        .flat_map(|i| (0..n).map(move |k| (i, k)))
        .filter(|(i, k)| (i + k) % 2 == 0)
        .map(|(i, k)| {
            let c = (i as Real + 0.5) * cell;
            let d = (k as Real + 0.5) * cell;
            plane.project(&Point3::new(c, d, 3.0))
        })
        .collect();
    cut_regions(&mut part, j, |r| {
        r.holes.is_empty()
            && r.area() < hole * hole * 1.01
            && centres.iter().any(|c| r.contains(*c))
    })?;
    Ok(part)
}

/// A strict checkerboard: holes as big as the cells touch at their
/// corners (non-manifold edges). Must be refused cleanly, never hang.
fn c_checker_touching() -> Result<PartDocument, String> {
    checker_grille(4, 10.0, 10.0)
}

/// A cut pointing away from the body removes nothing: an error.
fn c_cut_away() -> Result<PartDocument, String> {
    let mut part = plate(40.0, 30.0, 10.0)?;
    let j = sketch_on_top(&mut part, "Extrusion1:End", 10.0, |sk, at| {
        sk.add_circle(at(20.0, 15.0), 5.0)
            .map(|_| ())
            .map_err(|e| e.to_string())
    })?;
    let regions = part.sketches[j].sketch.regions();
    let r = regions
        .iter()
        .find(|r| r.holes.is_empty() && r.area() < 100.0)
        .ok_or("no region")?;
    let mut ex = ExtrudeFeature::new(vec![ProfileRef::of(r, r.interior_point())], 5.0);
    ex.operation = Operation::Cut;
    part.add_extrude(j, ex).map_err(|e| e.to_string())?;
    Ok(part)
}

/// Edit Feature: the hole's cut changed from through-all to 4 deep; the
/// fillet on the top edge after it survives.
fn c_edit_feature() -> Result<PartDocument, String> {
    let mut part = s_through_hole()?;
    let e = edges_where(&part, |h| (h[2] - 10.0).abs() < 1e-6 && h[1].abs() < 1e-6);
    part.add_blend(BlendFeature::fillet(e, 2.0))
        .map_err(|e| e.to_string())?;
    let k = part
        .features
        .iter()
        .position(|f| f.id() == "Extrusion2")
        .ok_or("no Extrusion2")?;
    let Some(Feature::Extrude(mut ex)) = part.features.get(k).cloned() else {
        return Err("not an extrude".into());
    };
    ex.extent = Extent::Distance;
    ex.distance = 4.0;
    part.replace_feature(k, Feature::Extrude(ex))
        .map_err(|e| e.to_string())?;
    if !part.feature_health.is_empty() {
        return Err(format!("sick: {:?}", part.feature_health));
    }
    Ok(part)
}

// -------------------------------------------------------------- shell ---

/// The reference of the face named `name` (the piece nearest `at`).
fn face_at(part: &PartDocument, name: &str, at: [Real; 3]) -> Result<FaceRef, String> {
    let p = Point3::from(at);
    part.body
        .faces()
        .filter(|(_, f)| f.name.as_deref() == Some(name))
        .filter_map(|(fid, _)| cadcraft3d::part::shell::face_ref(&part.body, fid, Some(p)))
        .next()
        .ok_or_else(|| format!("no face {name}"))
}

fn shell_of(
    mut part: PartDocument,
    faces: &[(&str, [Real; 3])],
    t: Real,
    dir: ShellDirection,
) -> Result<PartDocument, String> {
    let mut def = ShellFeature::new(t);
    def.direction = dir;
    for (n, at) in faces {
        def.remove.push(face_at(&part, n, *at)?);
    }
    part.add_shell(def).map_err(|e| e.to_string())?;
    Ok(part)
}

const TOP: (&str, [Real; 3]) = ("Extrusion1:End", [20.0, 15.0, 20.0]);

fn s_shell_box() -> Result<PartDocument, String> {
    shell_of(
        plate(40.0, 30.0, 20.0)?,
        &[TOP],
        2.0,
        ShellDirection::Inside,
    )
}

fn s_shell_outside() -> Result<PartDocument, String> {
    shell_of(
        plate(40.0, 30.0, 20.0)?,
        &[TOP],
        2.0,
        ShellDirection::Outside,
    )
}

fn s_shell_both() -> Result<PartDocument, String> {
    shell_of(plate(40.0, 30.0, 20.0)?, &[TOP], 2.0, ShellDirection::Both)
}

fn s_shell_cylinder() -> Result<PartDocument, String> {
    shell_of(
        s_cylinder()?,
        &[("Extrusion1:End", [0.0, 0.0, 50.0])],
        2.0,
        ShellDirection::Inside,
    )
}

fn s_shell_closed() -> Result<PartDocument, String> {
    shell_of(plate(40.0, 30.0, 20.0)?, &[], 2.0, ShellDirection::Inside)
}

/// The four vertical edges of the 40×30×20 plate (every piece: after a
/// shell they are split where the floor inside is, the blend joins them).
fn vertical_edges(part: &PartDocument) -> Vec<EdgeRef> {
    edges_where(part, |h| {
        h[2] > 1e-6
            && h[2] < 20.0 - 1e-6
            && (h[0].abs() < 1e-6 || (h[0] - 40.0).abs() < 1e-6)
            && (h[1].abs() < 1e-6 || (h[1] - 30.0).abs() < 1e-6)
    })
}

/// Fillet r5 on the vertical edges, then a shell open at the top.
fn c_fillet_then_shell() -> Result<PartDocument, String> {
    let mut part = plate(40.0, 30.0, 20.0)?;
    let e = vertical_edges(&part);
    if e.len() != 4 {
        return Err(format!("{} vertical edges", e.len()));
    }
    part.add_blend(BlendFeature::fillet(e, 5.0))
        .map_err(|e| e.to_string())?;
    shell_of(part, &[TOP], 2.0, ShellDirection::Inside)
}

/// A shell, then a fillet r1 on the outer vertical edges.
fn c_shell_then_fillet() -> Result<PartDocument, String> {
    let mut part = s_shell_box()?;
    let e = vertical_edges(&part);
    if e.len() != 8 {
        return Err(format!("{} vertical edge pieces", e.len()));
    }
    part.add_blend(BlendFeature::fillet(e, 1.0))
        .map_err(|e| e.to_string())?;
    Ok(part)
}

/// A shell, then a chamfer 0.5 round the outer bottom edges.
fn c_shell_then_chamfer() -> Result<PartDocument, String> {
    let mut part = s_shell_box()?;
    let e = edges_where(&part, |h| {
        h[2].abs() < 1e-6
            && (h[0].abs() < 1e-6
                || (h[0] - 40.0).abs() < 1e-6
                || h[1].abs() < 1e-6
                || (h[1] - 30.0).abs() < 1e-6)
    });
    if e.len() != 4 {
        return Err(format!("{} bottom edges", e.len()));
    }
    part.add_blend(BlendFeature::chamfer(e, 0.5))
        .map_err(|e| e.to_string())?;
    Ok(part)
}

/// A shell, then a hole Ø6 cut through the front wall (a sketch on the
/// front face).
fn c_shell_then_hole() -> Result<PartDocument, String> {
    let mut part = s_shell_box()?;
    let front = part
        .body
        .faces()
        .find(|(_, f)| {
            f.name
                .as_deref()
                .is_some_and(|n| n.starts_with("Extrusion1:Side:"))
                && f.mesh
                    .as_ref()
                    .is_some_and(|m| m.positions.iter().all(|p| p.y.abs() < 1e-6))
        })
        .and_then(|(_, f)| f.name.clone())
        .ok_or("no front face")?;
    let j = part
        .create_sketch(SketchPlacement::on_face(&front), &no_projection())
        .map_err(|e| e.to_string())?;
    let plane = part.sketches[j].sketch.plane.clone();
    let c = plane.project(&Point3::new(20.0, 0.0, 10.0));
    part.sketches[j]
        .sketch
        .add_circle(c, 3.0)
        .map_err(|e| e.to_string())?;
    let regions = part.sketches[j].sketch.regions();
    let r = regions
        .iter()
        .find(|r| r.holes.is_empty() && r.area() < 30.0)
        .ok_or("no region")?;
    let mut ex = ExtrudeFeature::new(vec![ProfileRef::of(r, r.interior_point())], 1.0);
    ex.operation = Operation::Cut;
    ex.extent = Extent::ThroughAll;
    ex.direction = ExtentDirection::Flipped;
    // Through the front wall only: 2 deep into the box.
    ex.extent = Extent::Distance;
    ex.distance = 2.0;
    part.add_extrude(j, ex).map_err(|e| e.to_string())?;
    Ok(part)
}

/// A shell open at the top and the front.
fn c_shell_two_faces() -> Result<PartDocument, String> {
    let part = plate(40.0, 30.0, 20.0)?;
    let front = part
        .body
        .faces()
        .find(|(_, f)| {
            f.mesh
                .as_ref()
                .is_some_and(|m| m.positions.iter().all(|p| p.y.abs() < 1e-6))
        })
        .and_then(|(_, f)| f.name.clone())
        .ok_or("no front face")?;
    shell_of(
        part,
        &[TOP, (front.as_str(), [20.0, 0.0, 10.0])],
        2.0,
        ShellDirection::Inside,
    )
}

/// Edit Feature: the shell's thickness 2 → 3; a fillet after it survives.
fn c_shell_edit() -> Result<PartDocument, String> {
    let mut part = c_shell_then_fillet()?;
    let k = part
        .features
        .iter()
        .position(|f| f.id() == "Shell1")
        .ok_or("no Shell1")?;
    let Some(Feature::Shell(mut sh)) = part.features.get(k).cloned() else {
        return Err("not a shell".into());
    };
    sh.thickness = 3.0;
    part.replace_feature(k, Feature::Shell(sh))
        .map_err(|e| e.to_string())?;
    if !part.feature_health.is_empty() {
        return Err(format!("sick: {:?}", part.feature_health));
    }
    Ok(part)
}

/// SolidCraft #2178: a shell after an extrusion tapered −5°.
fn c_shell_after_taper() -> Result<PartDocument, String> {
    let part = rect_extrude(|e| e.taper = (-5f64).to_radians())?;
    shell_of(part, &[TOP], 2.0, ShellDirection::Inside)
}

/// Volume of the −5° tapered 40×30×20 block shelled 2 (open top): the
/// walls offset 2 square to themselves (2 / cos 5° across), the floor 2 up.
fn taper_shell_volume() -> Real {
    let (k, c) = (5f64.to_radians().tan(), 5f64.to_radians().cos());
    let section =
        |z: Real, inset: Real| (40.0 - 2.0 * z * k - inset) * (30.0 - 2.0 * z * k - inset);
    // Simpson's rule is exact for these quadratics.
    let integral = |f: &dyn Fn(Real) -> Real, a: Real, b: Real| {
        (b - a) / 6.0 * (f(a) + 4.0 * f((a + b) / 2.0) + f(b))
    };
    integral(&|z| section(z, 0.0), 0.0, 20.0) - integral(&|z| section(z, 4.0 / c), 2.0, 20.0)
}

/// A disc r20 × 5 with its top circular edge rounded `r`.
fn rounded_disc(r: Real) -> Result<PartDocument, String> {
    let mut part = PartDocument::default();
    let i = part
        .create_sketch(
            SketchPlacement::on_origin_plane(OriginPlane::XY),
            &no_projection(),
        )
        .map_err(|e| e.to_string())?;
    part.sketches[i]
        .sketch
        .add_circle(Point2::new(0.0, 0.0), 20.0)
        .map_err(|e| e.to_string())?;
    let reg = part.sketches[i].sketch.regions();
    part.add_extrude(
        i,
        ExtrudeFeature::new(vec![ProfileRef::of(&reg[0], reg[0].interior_point())], 5.0),
    )
    .map_err(|e| e.to_string())?;
    let e = edges_where(&part, |h| (h[2] - 5.0).abs() < 1e-6);
    if e.is_empty() {
        return Err("no top edge".into());
    }
    part.add_blend(BlendFeature::fillet(e, r))
        .map_err(|e| e.to_string())?;
    Ok(part)
}

/// SolidCraft #319: the disc's top edge rounded r1 (less than the wall),
/// shelled 2 with the bottom removed: the inner corner comes out sharp.
fn c_shell_disc_small_round() -> Result<PartDocument, String> {
    let part = rounded_disc(1.0)?;
    shell_of(
        part,
        &[("Extrusion1:Start", [5.0, 0.0, 0.0])],
        2.0,
        ShellDirection::Inside,
    )
}

/// SolidCraft #342: the round equals the wall and meets the removed top:
/// the rim would have no thickness, refused.
fn c_shell_disc_round_as_thick() -> Result<PartDocument, String> {
    let part = rounded_disc(2.0)?;
    shell_of(
        part,
        &[("Extrusion1:End", [0.0, 0.0, 5.0])],
        2.0,
        ShellDirection::Inside,
    )
}

/// The #1494 part shelled 1: the union less the union of the box and the
/// cylinder each moved in by 1 (the cavity is a little larger along the two
/// concave seams, where the eroded union gets rounds; within 30 mm³).
fn edge_cylinder_shell_volume() -> Real {
    let (r, ri) = (7.5, 6.5);
    let union = 47.5 * 45.0 * 30.0 + 0.75 * PI * r * r * 47.5;
    let inner_box = 45.5 * 43.0 * 29.0;
    let inner_cyl = PI * ri * ri * 45.5;
    // The inner cylinder's part inside the inner box: its disc where
    // y ≥ 1 and z ≥ 1 (numerically).
    let n = 20_000;
    let y1 = (ri * ri - 1.0_f64).sqrt();
    let h = (y1 - 1.0) / n as Real;
    let quad: Real = (0..n)
        .map(|i| {
            let y = 1.0 + (i as Real + 0.5) * h;
            ((ri * ri - y * y).sqrt() - 1.0) * h
        })
        .sum();
    union - (inner_box + inner_cyl - quad * 45.5)
}

/// SolidCraft #1494: a box 47.5×45×30 joined with a cylinder r7.5 whose
/// axis is the box's bottom front edge, shelled 1 with the top removed.
fn c_shell_box_with_edge_cylinder() -> Result<PartDocument, String> {
    let mut part = plate(47.5, 45.0, 30.0)?;
    let j = part
        .create_sketch(
            SketchPlacement::on_origin_plane(OriginPlane::YZ),
            &no_projection(),
        )
        .map_err(|e| e.to_string())?;
    part.sketches[j]
        .sketch
        .add_circle(Point2::new(0.0, 0.0), 7.5)
        .map_err(|e| e.to_string())?;
    let reg = part.sketches[j].sketch.regions();
    let disc = reg
        .iter()
        .find(|r| r.holes.is_empty() && (r.area() - PI * 56.25).abs() < 1.0)
        .ok_or("no disc")?;
    let mut ex = ExtrudeFeature::new(vec![ProfileRef::of(disc, disc.interior_point())], 47.5);
    ex.operation = Operation::Join;
    part.add_extrude(j, ex).map_err(|e| e.to_string())?;
    shell_of(
        part,
        &[("Extrusion1:End", [20.0, 20.0, 30.0])],
        1.0,
        ShellDirection::Inside,
    )
}

// -------------------------------------------------------------- draft ---

/// The name of the face all of whose display points satisfy `on`.
fn face_where(part: &PartDocument, on: impl Fn(&Point3) -> bool) -> Result<String, String> {
    part.body
        .faces()
        .find(|(_, f)| f.mesh.as_ref().is_some_and(|m| m.positions.iter().all(&on)))
        .and_then(|(_, f)| f.name.clone())
        .ok_or_else(|| "no such face".into())
}

/// The plate's four side faces as (name, a point near height `z`).
fn plate_sides(part: &PartDocument, z: Real) -> Result<Vec<(String, [Real; 3])>, String> {
    Ok(vec![
        (face_where(part, |p| p.y.abs() < 1e-6)?, [20.0, 0.0, z]),
        (
            face_where(part, |p| (p.y - 30.0).abs() < 1e-6)?,
            [20.0, 30.0, z],
        ),
        (face_where(part, |p| p.x.abs() < 1e-6)?, [0.0, 15.0, z]),
        (
            face_where(part, |p| (p.x - 40.0).abs() < 1e-6)?,
            [40.0, 15.0, z],
        ),
    ])
}

fn draft_of(
    mut part: PartDocument,
    def: impl FnOnce(&mut DraftFeature),
    faces: &[(String, [Real; 3])],
) -> Result<PartDocument, String> {
    let mut d = DraftFeature::new(5f64.to_radians());
    def(&mut d);
    for (n, at) in faces {
        d.faces.push(face_at(&part, n, *at)?);
    }
    part.add_draft(d).map_err(|e| e.to_string())?;
    Ok(part)
}

fn fixed_xy(d: &mut DraftFeature) {
    d.draft_type = DraftType::FixedPlane;
    d.plane = Some(PlaneRef::Origin(OriginPlane::XY));
}

fn pull_up(d: &mut DraftFeature) {
    d.draft_type = DraftType::FixedEdge;
    d.pull = Some(PullRef::Plane(PlaneRef::Origin(OriginPlane::XY)));
}

fn k5() -> Real {
    5f64.to_radians().tan()
}

fn simpson(f: &dyn Fn(Real) -> Real, a: Real, b: Real) -> Real {
    (b - a) / 6.0 * (f(a) + 4.0 * f((a + b) / 2.0) + f(b))
}

/// The plate drafted 5° on all sides about the bottom: a frustum.
fn frustum_volume() -> Real {
    simpson(
        &|z| (40.0 - 2.0 * z * k5()) * (30.0 - 2.0 * z * k5()),
        0.0,
        20.0,
    )
}

fn s_draft_fixed_plane() -> Result<PartDocument, String> {
    let part = plate(40.0, 30.0, 20.0)?;
    let sides = plate_sides(&part, 10.0)?;
    draft_of(part, fixed_xy, &sides)
}

/// Fixed Edge, faces clicked near their top edges: they lean out below.
fn s_draft_fixed_top_edges() -> Result<PartDocument, String> {
    let part = plate(40.0, 30.0, 20.0)?;
    let sides = plate_sides(&part, 19.0)?;
    draft_of(part, pull_up, &sides)
}

fn s_draft_one_face() -> Result<PartDocument, String> {
    let part = plate(40.0, 30.0, 20.0)?;
    let front = plate_sides(&part, 1.0)?.remove(0);
    draft_of(part, pull_up, &[front])
}

fn s_draft_cylinder() -> Result<PartDocument, String> {
    let part = s_cylinder()?;
    let wall = face_where(&part, |p| (p.x * p.x + p.y * p.y - 225.0).abs() < 1e-3)?;
    draft_of(part, fixed_xy, &[(wall, [15.0, 0.0, 25.0])])
}

/// The front fixed at its bottom edge, the right side at its top edge: two
/// neutral planes, drafted one after the other.
fn c_draft_two_fixed_edges() -> Result<PartDocument, String> {
    let part = plate(40.0, 30.0, 20.0)?;
    let s = plate_sides(&part, 1.0)?;
    let front = s[0].clone();
    let right = (s[3].0.clone(), [40.0, 15.0, 19.0]);
    draft_of(part, pull_up, &[front, right])
}

/// Vertical edges rounded r5, then all side faces drafted (the chain of
/// planes and rounds): a tapered rounded rectangle.
fn c_fillet_then_draft() -> Result<PartDocument, String> {
    let mut part = plate(40.0, 30.0, 20.0)?;
    let e = vertical_edges(&part);
    part.add_blend(BlendFeature::fillet(e, 5.0))
        .map_err(|e| e.to_string())?;
    let mut faces: Vec<(String, [Real; 3])> = Vec::new();
    for (_, f) in part.body.faces() {
        let (Some(n), Some(m)) = (&f.name, &f.mesh) else {
            continue;
        };
        let vertical = m.normals.iter().all(|v| v.z.abs() < 1e-6);
        if vertical && let Some(p) = m.positions.first() {
            faces.push((n.clone(), [p.x, p.y, 10.0_f64.min(p.z.max(1.0))]));
        }
    }
    let mut out = Vec::new();
    for (n, _) in &faces {
        let (_, f) = part
            .body
            .faces()
            .find(|(_, f)| f.name.as_deref() == Some(n))
            .ok_or("face")?;
        let m = f.mesh.as_ref().ok_or("mesh")?;
        let t = m.triangles.first().ok_or("tri")?;
        let c = t.map(|i| m.positions[i as usize].coords);
        let c = (c[0] + c[1] + c[2]) / 3.0;
        out.push((n.clone(), [c.x, c.y, c.z]));
    }
    if out.len() != 8 {
        return Err(format!("{} side faces", out.len()));
    }
    draft_of(part, fixed_xy, &out)
}

fn rounded_frustum_volume() -> Real {
    let area = |z: Real| {
        let d = z * k5();
        (40.0 - 2.0 * d) * (30.0 - 2.0 * d) - (4.0 - PI) * (5.0 - d) * (5.0 - d)
    };
    simpson(&area, 0.0, 20.0)
}

/// All sides drafted 5°, then the top outline rounded r1.
fn c_draft_then_fillet() -> Result<PartDocument, String> {
    let mut part = s_draft_fixed_plane()?;
    let e = edges_where(&part, |h| (h[2] - 20.0).abs() < 1e-6);
    if e.len() != 4 {
        return Err(format!("{} top edges", e.len()));
    }
    part.add_blend(BlendFeature::fillet(e, 1.0))
        .map_err(|e| e.to_string())?;
    Ok(part)
}

/// The top edges' rounds r1 on the 5° frustum: the cross-section lost at a
/// 95° edge, along the top outline (corner patches ignored: ~r³).
fn draft_fillet_volume() -> Real {
    let phi = 95f64.to_radians();
    let lost = 1.0 / (phi / 2.0).tan() - (PI - phi) / 2.0;
    let (a, b) = (40.0 - 40.0 * k5(), 30.0 - 40.0 * k5());
    frustum_volume() - lost * 2.0 * (a + b)
}

fn c_draft_then_shell() -> Result<PartDocument, String> {
    let part = s_draft_fixed_plane()?;
    shell_of(part, &[TOP], 2.0, ShellDirection::Inside)
}

/// A shell, then its outer sides drafted about the bottom: the walls get
/// thinner upwards (0.25 left at the top), the cavity stays.
fn c_shell_then_draft() -> Result<PartDocument, String> {
    let shelled = s_shell_box()?;
    let sides = plate_sides(&plate(40.0, 30.0, 20.0)?, 10.0)?;
    draft_of(shelled, fixed_xy, &sides)
}

/// Edit Feature: the draft 5° → 3°; the fillet after it is kept.
fn c_draft_edit() -> Result<PartDocument, String> {
    let mut part = c_draft_then_fillet()?;
    let k = part
        .features
        .iter()
        .position(|f| f.id() == "FaceDraft1")
        .ok_or("no FaceDraft1")?;
    let Some(Feature::Draft(mut d)) = part.features.get(k).cloned() else {
        return Err("not a draft".into());
    };
    d.angle = 3f64.to_radians();
    part.replace_feature(k, Feature::Draft(d))
        .map_err(|e| e.to_string())?;
    if !part.feature_health.is_empty() {
        return Err(format!("sick: {:?}", part.feature_health));
    }
    Ok(part)
}

fn c_draft_bad_angle() -> Result<PartDocument, String> {
    let part = plate(40.0, 30.0, 20.0)?;
    let sides = plate_sides(&part, 10.0)?;
    draft_of(
        part,
        |d| {
            fixed_xy(d);
            d.angle = 95f64.to_radians();
        },
        &sides,
    )
}

/// The owner's case: every edge of the 40×30×20 plate rounded `r`, shelled
/// 2 with the top removed (r ≥ the wall: the kernel refuses the rim, the
/// opening is cut as a prism of the top face).
fn rounded_box_shell(r: Real) -> Result<PartDocument, String> {
    let mut part = plate(40.0, 30.0, 20.0)?;
    let all = edges_where(&part, |_| true);
    if all.len() != 12 {
        return Err(format!("{} edges", all.len()));
    }
    part.add_blend(BlendFeature::fillet(all, r))
        .map_err(|e| e.to_string())?;
    shell_of(part, &[TOP], 2.0, ShellDirection::Inside)
}

fn c_rounded_box_shell_r2() -> Result<PartDocument, String> {
    rounded_box_shell(2.0)
}

fn c_rounded_box_shell_r3() -> Result<PartDocument, String> {
    rounded_box_shell(3.0)
}

/// A box `a × b × c` with every edge rounded `r` (Steiner's formula).
fn rounded_volume(a: Real, b: Real, c: Real, r: Real) -> Real {
    let (p, q, s) = (a - 2.0 * r, b - 2.0 * r, c - 2.0 * r);
    p * q * s
        + 2.0 * (p * q + q * s + s * p) * r
        + PI * (p + q + s) * r * r
        + 4.0 / 3.0 * PI * r * r * r
}

/// Its volume: the outside less the closed cavity less the top wall under
/// the top face.
fn rounded_box_shell_volume(r: Real) -> Real {
    rounded_volume(40.0, 30.0, 20.0, r)
        - rounded_volume(36.0, 26.0, 16.0, r - 2.0)
        - (40.0 - 2.0 * r) * (30.0 - 2.0 * r) * 2.0
}

/// Walls thicker than half the box: refused, the part unchanged.
fn c_shell_too_thick() -> Result<PartDocument, String> {
    shell_of(
        plate(40.0, 30.0, 20.0)?,
        &[TOP],
        16.0,
        ShellDirection::Inside,
    )
}

// ------------------------------------------------------- very complex ---

fn v_checker_grille_12() -> Result<PartDocument, String> {
    checker_grille(12, 8.0, 6.0)
}

fn v_radial_grille_24() -> Result<PartDocument, String> {
    radial_grille(24, 40.0, 34.0)
}

/// A 6×6 grid of Ø4 holes in a plate, every hole's top edge chamfered 0.5.
fn v_holes_chamfered() -> Result<PartDocument, String> {
    let mut part = plate(60.0, 60.0, 6.0)?;
    let j = sketch_on_top(&mut part, "Extrusion1:End", 6.0, |sk, at| {
        for i in 0..6 {
            for k in 0..6 {
                sk.add_circle(at(5.0 + 10.0 * i as Real, 5.0 + 10.0 * k as Real), 2.0)
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    })?;
    cut_regions(&mut part, j, |r| r.holes.is_empty() && r.area() < 13.0)?;
    let e = edges_where(&part, |h| {
        (h[2] - 6.0).abs() < 1e-6 && h[0] > 1.0 && h[0] < 59.0 && h[1] > 1.0 && h[1] < 59.0
    });
    if e.len() < 36 {
        return Err(format!("{} hole edges", e.len()));
    }
    part.add_blend(BlendFeature::chamfer(e, 0.5))
        .map_err(|e| e.to_string())?;
    Ok(part)
}

/// The same plate and holes, every hole's top edge rounded r 0.5.
fn v_holes_filleted() -> Result<PartDocument, String> {
    let mut part = plate(60.0, 60.0, 6.0)?;
    let j = sketch_on_top(&mut part, "Extrusion1:End", 6.0, |sk, at| {
        for i in 0..6 {
            for k in 0..6 {
                sk.add_circle(at(5.0 + 10.0 * i as Real, 5.0 + 10.0 * k as Real), 2.0)
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    })?;
    cut_regions(&mut part, j, |r| r.holes.is_empty() && r.area() < 13.0)?;
    let e = edges_where(&part, |h| {
        (h[2] - 6.0).abs() < 1e-6 && h[0] > 1.0 && h[0] < 59.0 && h[1] > 1.0 && h[1] < 59.0
    });
    part.add_blend(BlendFeature::fillet(e, 0.5))
        .map_err(|e| e.to_string())?;
    Ok(part)
}

// ------------------------------------------------- pocket with rounds ---

/// The pocket's outline in plate coordinates: two end circles r 10 at
/// (30, 30) and (70, 30), joined below by a line and above by an arc of
/// radius 40 tangent to both (`dent` false: bulging out) or of radius 20
/// curving into the pocket (`dent` true). Pieces as (kind, data) for the
/// sketch and for the analytic area/perimeter: lines (a, b), arcs CCW
/// (centre, radius, from angle, to angle).
#[derive(Clone, Copy)]
enum Piece {
    Line([Real; 2], [Real; 2]),
    Arc([Real; 2], Real, Real, Real),
}

fn pocket_outline(dent: bool) -> Vec<Piece> {
    let (c1, c2, r) = ([30.0, 30.0], [70.0, 30.0], 10.0);
    let ang = |c: [Real; 2], p: [Real; 2]| (p[1] - c[1]).atan2(p[0] - c[0]);
    let norm = |a: Real, b: Real| {
        // CCW sweep from a to b in (0, 2π].
        let mut s = b - a;
        while s <= 0.0 {
            s += 2.0 * PI;
        }
        a + s
    };
    let (big, rr) = if dent { (20.0, 30.0) } else { (40.0, 30.0) };
    // |C − c_i| = rr (40 − 10 inside, 20 + 10 outside).
    let h = (rr * rr - 400.0_f64).sqrt();
    let cc = if dent {
        [50.0, 30.0 + h]
    } else {
        [50.0, 30.0 - h]
    };
    let toward = |from: [Real; 2], to: [Real; 2], len: Real| {
        let d = [to[0] - from[0], to[1] - from[1]];
        let n = (d[0] * d[0] + d[1] * d[1]).sqrt();
        [from[0] + d[0] / n * len, from[1] + d[1] / n * len]
    };
    // Touching points on the end circles.
    let (t1, t2) = if dent {
        (toward(c1, cc, r), toward(c2, cc, r))
    } else {
        (toward(cc, c1, big), toward(cc, c2, big))
    };
    let b1 = [30.0, 20.0];
    let b2 = [70.0, 20.0];
    let a2 = ang(c2, b2);
    let a1 = ang(c1, t1);
    let mut out = vec![
        Piece::Line(b1, b2),
        Piece::Arc(c2, r, a2, norm(a2, ang(c2, t2))),
    ];
    if dent {
        // Traversed clockwise round its centre: as a CCW arc from t1 to t2.
        let s = ang(cc, t1);
        out.push(Piece::Arc(cc, big, s, norm(s, ang(cc, t2))));
    } else {
        let s = ang(cc, t2);
        out.push(Piece::Arc(cc, big, s, norm(s, ang(cc, t1))));
    }
    out.push(Piece::Arc(c1, r, a1, norm(a1, ang(c1, b1))));
    out
}

/// Area and perimeter of the outline (Green's theorem; the dent arc is
/// walked backwards along the boundary).
fn pocket_area_perimeter(dent: bool) -> (Real, Real) {
    let mut area = 0.0;
    let mut len = 0.0;
    for (k, p) in pocket_outline(dent).into_iter().enumerate() {
        match p {
            Piece::Line(a, b) => {
                area += 0.5 * (a[0] * b[1] - b[0] * a[1]);
                len += ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
            }
            Piece::Arc(c, r, t0, t1) => {
                let term = 0.5
                    * (r * r * (t1 - t0) + c[0] * r * (t1.sin() - t0.sin())
                        - c[1] * r * (t1.cos() - t0.cos()));
                let backwards = dent && k == 2;
                area += if backwards { -term } else { term };
                len += r * (t1 - t0);
            }
        }
    }
    (area, len)
}

/// A 100×60×12 plate with the pocket cut 8 deep from the top.
fn pocket_plate(dent: bool) -> Result<PartDocument, String> {
    let mut part = plate(100.0, 60.0, 12.0)?;
    let j = sketch_on_top(&mut part, "Extrusion1:End", 12.0, |sk, at| {
        // Sketch frames may be mirrored against the model: arcs are CCW in
        // the sketch, so swap their ends then.
        let (o, x, y) = (at(0.0, 0.0), at(1.0, 0.0), at(0.0, 1.0));
        let mirrored = (x - o).perp(&(y - o)) < 0.0;
        let mut pts: Vec<(Point2, cadcraft3d::sketch::PointId)> = Vec::new();
        let mut point = |sk: &mut cadcraft3d::sketch::Sketch, q: [Real; 2]| {
            let p = at(q[0], q[1]);
            if let Some((_, id)) = pts.iter().find(|(x, _)| (x - p).norm() < 1e-6) {
                return *id;
            }
            let id = sk.add_point(p);
            pts.push((p, id));
            id
        };
        for piece in pocket_outline(dent) {
            match piece {
                Piece::Line(a, b) => {
                    let (pa, pb) = (point(sk, a), point(sk, b));
                    sk.add_line_between(pa, pb).map_err(|e| e.to_string())?;
                }
                Piece::Arc(c, r, t0, t1) => {
                    let pc = sk.add_point(at(c[0], c[1]));
                    let pa = point(sk, [c[0] + r * t0.cos(), c[1] + r * t0.sin()]);
                    let pb = point(sk, [c[0] + r * t1.cos(), c[1] + r * t1.sin()]);
                    let (s, e) = if mirrored { (pb, pa) } else { (pa, pb) };
                    sk.add_arc(pc, s, e).map_err(|e| e.to_string())?;
                }
            }
        }
        Ok(())
    })?;
    let regions = part.sketches[j].sketch.regions();
    let pocket: Vec<ProfileRef> = regions
        .iter()
        .filter(|r| r.holes.is_empty())
        .map(|r| ProfileRef::of(r, r.interior_point()))
        .collect();
    if pocket.len() != 1 {
        return Err(format!(
            "{} pocket regions of {}",
            pocket.len(),
            regions.len()
        ));
    }
    let mut ex = ExtrudeFeature::new(pocket, 8.0);
    ex.operation = Operation::Cut;
    ex.direction = ExtentDirection::Flipped;
    part.add_extrude(j, ex).map_err(|e| e.to_string())?;
    Ok(part)
}

/// Edges of the pocket at height `z` (inside the plate outline).
fn pocket_edges(part: &PartDocument, z: Real) -> Vec<EdgeRef> {
    edges_where(part, |h| {
        (h[2] - z).abs() < 1e-6 && h[0] > 10.0 && h[0] < 90.0 && h[1] > 5.0 && h[1] < 58.0
    })
}

fn pocket_blend(dent: bool, rim: bool, floor: bool, chamfer: bool) -> Result<PartDocument, String> {
    let mut part = pocket_plate(dent)?;
    let mut e = Vec::new();
    if rim {
        e.extend(pocket_edges(&part, 12.0));
    }
    if floor {
        e.extend(pocket_edges(&part, 4.0));
    }
    if e.len() < 4 {
        return Err(format!("{} pocket edges", e.len()));
    }
    let def = if chamfer {
        BlendFeature::chamfer(e, 1.5)
    } else {
        BlendFeature::fillet(e, 2.0)
    };
    part.add_blend(def).map_err(|e| e.to_string())?;
    Ok(part)
}

fn c_pocket() -> Result<PartDocument, String> {
    pocket_plate(false)
}
fn c_pocket_rim() -> Result<PartDocument, String> {
    pocket_blend(false, true, false, false)
}
fn c_pocket_floor() -> Result<PartDocument, String> {
    pocket_blend(false, false, true, false)
}
fn c_pocket_rim_chamfer() -> Result<PartDocument, String> {
    pocket_blend(false, true, false, true)
}
fn v_pocket_rim_and_floor() -> Result<PartDocument, String> {
    pocket_blend(false, true, true, false)
}
fn v_dent_pocket_rim() -> Result<PartDocument, String> {
    pocket_blend(true, true, false, false)
}
fn v_dent_pocket_floor() -> Result<PartDocument, String> {
    pocket_blend(true, false, true, false)
}

/// Two edge sets in one fillet: the pocket's rim at r 2, its floor at r 1.
fn v_pocket_two_sets() -> Result<PartDocument, String> {
    let mut part = pocket_plate(false)?;
    let mut def = BlendFeature::fillet(pocket_edges(&part, 12.0), 2.0);
    def.sets.push(FilletSet {
        edges: pocket_edges(&part, 4.0),
        radius: 1.0,
    });
    part.add_blend(def).map_err(|e| e.to_string())?;
    Ok(part)
}

/// The box 40×30×20, its front top edge (y = 0, z = 20, along x).
fn front_top_edge(part: &PartDocument) -> Result<EdgeRef, String> {
    edges_where(part, |h| h[1].abs() < 1e-6 && (h[2] - 20.0).abs() < 1e-6)
        .into_iter()
        .next()
        .ok_or_else(|| "no front top edge".to_owned())
}

/// Variable radius 2 → 5 along the box's front top edge.
fn c_variable_fillet() -> Result<PartDocument, String> {
    let mut part = plate(40.0, 30.0, 20.0)?;
    let edge = front_top_edge(&part)?;
    let def = BlendFeature::variable_fillet(
        vec![VariableRadius {
            edge,
            start: [0.0, 0.0, 20.0],
            points: vec![[0.0, 2.0], [1.0, 5.0]],
        }],
        false,
    );
    part.add_blend(def).map_err(|e| e.to_string())?;
    Ok(part)
}

/// Partial chamfer 2 on the box's front top edge: from 10 mm after its
/// x = 0 end, 15 mm long.
fn partial_chamfer(to_start: Real, length: Real) -> Result<PartDocument, String> {
    let mut part = plate(40.0, 30.0, 20.0)?;
    let edge = front_top_edge(&part)?;
    let mut def = BlendFeature::chamfer(vec![edge], 2.0);
    def.partial = Some(PartialChamfer {
        start: [0.0, 0.0, 20.0],
        to_start,
        length,
    });
    part.add_blend(def).map_err(|e| e.to_string())?;
    Ok(part)
}
fn c_partial_chamfer() -> Result<PartDocument, String> {
    partial_chamfer(10.0, 15.0)
}
fn c_partial_chamfer_too_long() -> Result<PartDocument, String> {
    partial_chamfer(30.0, 15.0)
}

/// Chord width 4 on the box's front top edge (90°: r = 4 / √2).
fn c_chord_fillet() -> Result<PartDocument, String> {
    let mut part = plate(40.0, 30.0, 20.0)?;
    let edge = front_top_edge(&part)?;
    part.add_blend(BlendFeature::chord_fillet(vec![edge], 4.0))
        .map_err(|e| e.to_string())?;
    Ok(part)
}

/// Full Round of a 14×6×10 rib's end face (6 wide): r 3 on its two
/// vertical edges, the face is used up.
fn c_full_round() -> Result<PartDocument, String> {
    let mut part = plate(14.0, 6.0, 10.0)?;
    let end = part
        .body
        .faces()
        .find_map(|(f, x)| {
            let p = part.body.face_plane(f)?;
            (p.normal.x > 0.99 && (p.origin.x - 14.0).abs() < 1e-6).then(|| x.name.clone())?
        })
        .ok_or("no end face")?;
    part.add_blend(BlendFeature::full_round(end))
        .map_err(|e| e.to_string())?;
    Ok(part)
}

/// The pocket's floor (concave) chamfered 1.
fn c_pocket_floor_chamfer() -> Result<PartDocument, String> {
    let mut part = pocket_plate(false)?;
    let e = pocket_edges(&part, 4.0);
    part.add_blend(BlendFeature::chamfer(e, 1.0))
        .map_err(|e| e.to_string())?;
    Ok(part)
}

/// Select Mode Solid, All Rounds: every convex edge of the pocket plate
/// (its 12 edges and the pocket's rim) rounded r 1.
fn v_all_rounds() -> Result<PartDocument, String> {
    use cadcraft3d::part::select::{SolidEdges, solid_edges};
    let mut part = pocket_plate(false)?;
    let edges: Vec<EdgeRef> = solid_edges(&part.body, None, SolidEdges::Rounds)
        .into_iter()
        .filter_map(|e| edge_ref(&part.body, e))
        .collect();
    if edges.len() < 16 {
        return Err(format!("{} convex edges", edges.len()));
    }
    part.add_blend(BlendFeature::fillet(edges, 1.0))
        .map_err(|e| e.to_string())?;
    Ok(part)
}

/// A box a×b×c with all 12 edges rounded r: the edge strips between the
/// corners and the 8 corner pieces (a cube r³ less an eighth of a ball).
fn rounded_box_loss(a: Real, b: Real, c: Real, r: Real) -> Real {
    (1.0 - PI / 4.0) * r * r * 4.0 * ((a - 2.0 * r) + (b - 2.0 * r) + (c - 2.0 * r))
        + 8.0 * (r * r * r - PI * r * r * r / 6.0)
}

/// Volumes: plate − pocket, then the rounds by Pappus: spandrel area
/// (1 − π/4) r² swept along the outline offset by its centroid distance
/// d = r (10 − 3π) / (3 (4 − π)) — outwards at the rim (length L + 2π d,
/// removed), inwards at the floor (L − 2π d, added). Chamfer c: triangle
/// c²/2, centroid c/3 out.
fn pocket_volume(dent: bool, rim: bool, floor: bool, chamfer: bool) -> Real {
    let (a, l) = pocket_area_perimeter(dent);
    let mut v = 100.0 * 60.0 * 12.0 - a * 8.0;
    if chamfer {
        let c: Real = 1.5;
        v -= c * c / 2.0 * (l + 2.0 * PI * c / 3.0);
        return v;
    }
    let r: Real = 2.0;
    let af = (1.0 - PI / 4.0) * r * r;
    let d = r * (10.0 - 3.0 * PI) / (3.0 * (4.0 - PI));
    if rim {
        v -= af * (l + 2.0 * PI * d);
    }
    if floor {
        v += af * (l - 2.0 * PI * d);
    }
    v
}

/// Rim r 2 (removed) and floor r 1 (added) by Pappus, as in
/// [`pocket_volume`].
fn two_sets_volume() -> Real {
    let (a, l) = pocket_area_perimeter(false);
    let spandrel = |r: Real| {
        let d = r * (10.0 - 3.0 * PI) / (3.0 * (4.0 - PI));
        ((1.0 - PI / 4.0) * r * r, d)
    };
    let (a2, d2) = spandrel(2.0);
    let (a1, d1) = spandrel(1.0);
    100.0 * 60.0 * 12.0 - a * 8.0 - a2 * (l + 2.0 * PI * d2) + a1 * (l - 2.0 * PI * d1)
}

/// A 40×30 rectangle extruded with `f` setting the options.
fn rect_extrude(f: impl FnOnce(&mut ExtrudeFeature)) -> Result<PartDocument, String> {
    let mut part = PartDocument::default();
    let i = part
        .create_sketch(
            SketchPlacement::on_origin_plane(OriginPlane::XY),
            &no_projection(),
        )
        .map_err(|e| e.to_string())?;
    let sk = &mut part.sketches[i].sketch;
    sk.add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(40.0, 30.0))
        .map_err(|e| e.to_string())?;
    let r = sk.regions();
    let mut ex = ExtrudeFeature::new(vec![ProfileRef::of(&r[0], r[0].interior_point())], 20.0);
    f(&mut ex);
    part.add_extrude(i, ex).map_err(|e| e.to_string())?;
    Ok(part)
}

fn s_symmetric() -> Result<PartDocument, String> {
    rect_extrude(|e| e.direction = ExtentDirection::Symmetric)
}

fn s_asymmetric() -> Result<PartDocument, String> {
    rect_extrude(|e| {
        e.direction = ExtentDirection::Asymmetric;
        e.distance = 15.0;
        e.distance_b = 5.0;
    })
}

fn s_taper() -> Result<PartDocument, String> {
    rect_extrude(|e| e.taper = 5f64.to_radians())
}

/// The tube's section revolved 270° (Angle).
fn s_revolve_270() -> Result<PartDocument, String> {
    let mut part = PartDocument::default();
    let i = part
        .create_sketch(
            SketchPlacement::on_origin_plane(OriginPlane::XZ),
            &no_projection(),
        )
        .map_err(|e| e.to_string())?;
    let sk = &mut part.sketches[i].sketch;
    sk.add_rectangle_two_point(Point2::new(10.0, 0.0), Point2::new(20.0, 20.0))
        .map_err(|e| e.to_string())?;
    let r = sk.regions();
    let mut def = RevolveFeature::new(
        vec![ProfileRef::of(&r[0], r[0].interior_point())],
        cadcraft3d::part::AxisRef::Origin(2),
    );
    def.extent = cadcraft3d::part::RevolveExtent::Angle;
    def.angle = 1.5 * PI;
    part.add_revolve(i, def).map_err(|e| e.to_string())?;
    Ok(part)
}

/// A box and a cylinder r 15 through it, Intersect: the cylinder's part
/// inside the box.
fn c_intersect() -> Result<PartDocument, String> {
    let mut part = plate(40.0, 30.0, 20.0)?;
    let j = sketch_on_top(&mut part, "Extrusion1:End", 20.0, |sk, at| {
        sk.add_circle(at(20.0, 15.0), 15.0)
            .map(|_| ())
            .map_err(|e| e.to_string())
    })?;
    let regions = part.sketches[j].sketch.regions();
    let r = regions
        .iter()
        .find(|r| r.holes.is_empty() && (r.area() - PI * 225.0).abs() < 1.0)
        .ok_or("no circle region")?;
    let mut ex = ExtrudeFeature::new(vec![ProfileRef::of(r, r.interior_point())], 30.0);
    ex.operation = Operation::Intersect;
    ex.direction = ExtentDirection::Flipped;
    part.add_extrude(j, ex).map_err(|e| e.to_string())?;
    Ok(part)
}

/// A plate with a boss r 8 × 10 joined on top; the boss's base (a concave
/// circle) rounded r 2.
fn c_boss_base_fillet() -> Result<PartDocument, String> {
    let mut part = plate(40.0, 40.0, 5.0)?;
    let j = sketch_on_top(&mut part, "Extrusion1:End", 5.0, |sk, at| {
        sk.add_circle(at(20.0, 20.0), 8.0)
            .map(|_| ())
            .map_err(|e| e.to_string())
    })?;
    let regions = part.sketches[j].sketch.regions();
    let r = regions
        .iter()
        .find(|r| r.holes.is_empty() && r.area() < 210.0)
        .ok_or("no boss region")?;
    let mut ex = ExtrudeFeature::new(vec![ProfileRef::of(r, r.interior_point())], 10.0);
    ex.operation = Operation::Join;
    part.add_extrude(j, ex).map_err(|e| e.to_string())?;
    let e = edges_where(&part, |h| {
        (h[2] - 5.0).abs() < 1e-6 && ((h[0] - 20.0).powi(2) + (h[1] - 20.0).powi(2)).sqrt() < 9.0
    });
    if e.is_empty() {
        return Err("no base edge".into());
    }
    part.add_blend(BlendFeature::fillet(e, 2.0))
        .map_err(|e| e.to_string())?;
    Ok(part)
}

/// Chamfer, Two Distances 2 × 4, on one edge of the box.
fn s_chamfer_two() -> Result<PartDocument, String> {
    let mut part = plate(40.0, 30.0, 20.0)?;
    let e = edges_where(&part, |h| (h[2] - 20.0).abs() < 1e-6 && h[1].abs() < 1e-6);
    let mut c = BlendFeature::chamfer(e, 2.0);
    if let cadcraft3d::part::BlendKind::Chamfer {
        method, distance2, ..
    } = &mut c.kind
    {
        *method = cadcraft3d::part::ChamferMethod::TwoDistances;
        *distance2 = 4.0;
    }
    part.add_blend(c).map_err(|e| e.to_string())?;
    Ok(part)
}

/// A plate 40×30×10 and, as a separate body (New Solid), a slab over it
/// at z 20…25; returns the part.
fn plate_and_slab() -> Result<PartDocument, String> {
    let mut part = plate(40.0, 30.0, 10.0)?;
    let i = part
        .create_sketch(
            SketchPlacement::on_origin_plane(OriginPlane::XY),
            &no_projection(),
        )
        .map_err(|e| e.to_string())?;
    let sk = &mut part.sketches[i].sketch;
    sk.add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(40.0, 30.0))
        .map_err(|e| e.to_string())?;
    let r = sk.regions();
    let mut ex = ExtrudeFeature::new(vec![ProfileRef::of(&r[0], r[0].interior_point())], 5.0);
    ex.operation = Operation::NewSolid;
    ex.from = Some(cadcraft3d::part::TargetRef::Face("Extrusion1:End".into()));
    // From the plate's top, 10…15 above it: z 20…25 needs an offset start;
    // the plain way: extrude 25 and cut 0…20 is not a slab. Use From = top
    // face and Distance 15 then cut the lower 10 back with a second cut.
    ex.distance = 15.0;
    part.add_extrude(i, ex).map_err(|e| e.to_string())?;
    // Remove z 10…20 of the new body: a cut of the same rectangle from the
    // plate's top, 10 deep, on the new body only.
    let j = part
        .create_sketch(SketchPlacement::on_face("Extrusion1:End"), &no_projection())
        .map_err(|e| e.to_string())?;
    let plane = part.sketches[j].sketch.plane.clone();
    let a = plane.project(&Point3::new(-1.0, -1.0, 10.0));
    let b = plane.project(&Point3::new(41.0, 31.0, 10.0));
    part.sketches[j]
        .sketch
        .add_rectangle_two_point(a, b)
        .map_err(|e| e.to_string())?;
    let regions = part.sketches[j].sketch.regions();
    let big = regions
        .iter()
        .max_by(|x, y| x.area().total_cmp(&y.area()))
        .ok_or("no region")?;
    let mut cut = ExtrudeFeature::new(vec![ProfileRef::of(big, big.interior_point())], 10.0);
    cut.operation = Operation::Cut;
    cut.solids = vec!["Extrusion2".into()];
    part.add_extrude(j, cut).map_err(|e| e.to_string())?;
    Ok(part)
}

/// A circle r 5 on the plate's top, extruded with `extent` (Join).
fn post_to(extent: Extent) -> Result<PartDocument, String> {
    let mut part = plate_and_slab()?;
    let j = sketch_on_top(&mut part, "Extrusion1:End", 10.0, |sk, at| {
        sk.add_circle(at(20.0, 15.0), 5.0)
            .map(|_| ())
            .map_err(|e| e.to_string())
    })?;
    let regions = part.sketches[j].sketch.regions();
    let r = regions
        .iter()
        .find(|r| r.holes.is_empty() && (r.area() - PI * 25.0).abs() < 0.5)
        .ok_or("no circle region")?;
    let mut ex = ExtrudeFeature::new(vec![ProfileRef::of(r, r.interior_point())], 1.0);
    ex.operation = Operation::Join;
    ex.extent = extent;
    ex.solids = vec!["Extrusion1".into()];
    part.add_extrude(j, ex).map_err(|e| e.to_string())?;
    Ok(part)
}

fn c_extrude_to_face() -> Result<PartDocument, String> {
    // The slab's bottom face (z 20), facing the plate.
    post_to(Extent::To(cadcraft3d::part::TargetRef::Face(
        "Extrusion3:End".into(),
    )))
}

fn c_extrude_to_next() -> Result<PartDocument, String> {
    post_to(Extent::ToNext)
}

// --------------------------------------------------------------- hole ---

/// Holes `spec` at `centers` (model x, y, z on `face`) through a Hole
/// sketch on `face`.
fn hole_on(
    part: &mut PartDocument,
    face: &str,
    spec: HoleSpec,
    centers: &[[Real; 3]],
) -> Result<(), String> {
    let pts: Vec<Point3> = centers.iter().map(|c| Point3::from(*c)).collect();
    let (i, ids) = part
        .create_hole_sketch(face, &pts)
        .map_err(|e| e.to_string())?;
    let mut def = HoleFeature::new(spec);
    def.sketch = part.sketches[i].id.clone();
    def.centers = ids.into_iter().map(HoleCenter::Point).collect();
    part.add_hole(def).map_err(|e| e.to_string())?;
    Ok(())
}

fn through(d: Real) -> HoleSpec {
    let mut s = HoleSpec::simple(d, 10.0);
    s.termination = Termination::ThroughAll;
    s
}

/// The 40×30×20 plate with a Ø10 hole through its middle.
fn holed_plate() -> Result<PartDocument, String> {
    let mut part = plate(40.0, 30.0, 20.0)?;
    hole_on(
        &mut part,
        "Extrusion1:End",
        through(10.0),
        &[[20.0, 15.0, 20.0]],
    )?;
    Ok(part)
}

/// The hole's top rim (radius 5 at z 20).
fn rim(part: &PartDocument) -> Vec<EdgeRef> {
    edges_where(part, |h| {
        (h[2] - 20.0).abs() < 1e-6 && (h[0] - 20.0).hypot(h[1] - 15.0) < 5.01
    })
}

fn c_hole_then_fillet() -> Result<PartDocument, String> {
    let mut part = holed_plate()?;
    let e = rim(&part);
    if e.is_empty() {
        return Err("no rim edge".into());
    }
    part.add_blend(BlendFeature::fillet(e, 1.0))
        .map_err(|e| e.to_string())?;
    Ok(part)
}

fn c_hole_then_chamfer() -> Result<PartDocument, String> {
    let mut part = holed_plate()?;
    let e = rim(&part);
    if e.is_empty() {
        return Err("no rim edge".into());
    }
    part.add_blend(BlendFeature::chamfer(e, 0.5))
        .map_err(|e| e.to_string())?;
    Ok(part)
}

/// SolidCraft #559: a Ø1 hole into the 2 mm rim of a shelled box.
fn c_shell_then_hole_in_rim() -> Result<PartDocument, String> {
    let mut part = s_shell_box()?;
    let mut s = HoleSpec::simple(1.0, 5.0);
    s.drill_point = DrillPoint::Flat;
    hole_on(&mut part, "Extrusion1:End", s, &[[1.0, 15.0, 20.0]])?;
    Ok(part)
}

/// A Ø6 hole through the plate, then a shell open at the top: the bore
/// becomes a tube with 2 mm walls.
fn c_hole_then_shell() -> Result<PartDocument, String> {
    let mut part = plate(40.0, 30.0, 20.0)?;
    hole_on(
        &mut part,
        "Extrusion1:End",
        through(6.0),
        &[[20.0, 15.0, 20.0]],
    )?;
    shell_of(part, &[TOP], 2.0, ShellDirection::Inside)
}

/// A counterbored hole, edited to a larger counterbore.
fn c_hole_edit() -> Result<PartDocument, String> {
    let mut part = plate(40.0, 30.0, 20.0)?;
    let mut s = through(6.6);
    s.seat = SeatType::Counterbore;
    s.seat_diameter = 11.0;
    s.seat_depth = 6.4;
    hole_on(&mut part, "Extrusion1:End", s, &[[20.0, 15.0, 20.0]])?;
    let k = part.features.len() - 1;
    let Some(Feature::Hole(mut h)) = part.features.get(k).cloned() else {
        return Err("no hole".into());
    };
    h.spec.seat_diameter = 14.0;
    h.spec.seat_depth = 8.0;
    part.replace_feature(k, Feature::Hole(h))
        .map_err(|e| e.to_string())?;
    Ok(part)
}

fn cases() -> Vec<Case> {
    let cone_ring = |r: Real| PI * r * r * 0.5 * 0.5 / 3.0; // not used
    let _ = cone_ring;
    // Chamfer 0.5 on a hole edge of radius 2: the ring removed is the
    // revolved triangle, area 0.125 at radius 2 + 0.5/3.
    let ring = 2.0 * PI * (2.0 + 0.5 / 3.0) * 0.125;
    // Round r on a hole rim: area (1 − π/4) r², centroid 0.2234 r out.
    let round =
        2.0 * PI * (2.0 + 0.5 * (10.0 - 3.0 * PI) / (3.0 * (4.0 - PI))) * (1.0 - PI / 4.0) * 0.25;
    let shell_box = 24_000.0 - 36.0 * 26.0 * 18.0;
    let round4 = |r: Real, h: Real| 4.0 * (r * r - PI * r * r / 4.0) * h;
    // A round r on the rim of a hole of radius R: (1 − π/4) r² at
    // R + 0.2234 r; a chamfer c: c²/2 at R + c/3.
    let rim_round = |big: Real, r: Real| {
        2.0 * PI * (big + r * (10.0 - 3.0 * PI) / (3.0 * (4.0 - PI))) * (1.0 - PI / 4.0) * r * r
    };
    let rim_chamfer = |big: Real, c: Real| 2.0 * PI * (big + c / 3.0) * c * c / 2.0;
    let holed = 24_000.0 - PI * 25.0 * 20.0;
    vec![
        Case {
            level: "simple",
            name: "hole Ø10 through, fillet r1 on its rim",
            budget_ms: 1500.0,
            build: c_hole_then_fillet,
            want: Want::Volume(holed - rim_round(5.0, 1.0)),
        },
        Case {
            level: "simple",
            name: "hole Ø10 through, chamfer 0.5 on its rim",
            budget_ms: 1500.0,
            build: c_hole_then_chamfer,
            want: Want::Volume(holed - rim_chamfer(5.0, 0.5)),
        },
        Case {
            level: "complex",
            name: "shell, then a Ø1 hole into its rim (#559)",
            budget_ms: 2000.0,
            build: c_shell_then_hole_in_rim,
            want: Want::Volume(shell_box - PI * 0.25 * 5.0),
        },
        Case {
            level: "complex",
            name: "hole Ø6 through, then shell 2",
            budget_ms: 3000.0,
            build: c_hole_then_shell,
            want: Want::Volume(
                24_000.0 - PI * 9.0 * 20.0 - (36.0 * 26.0 * 18.0 - PI * 25.0 * 18.0),
            ),
        },
        Case {
            level: "simple",
            name: "counterbore edited Ø11×6.4 → Ø14×8",
            budget_ms: 1500.0,
            build: c_hole_edit,
            want: Want::Volume(24_000.0 - PI * (49.0 * 8.0 + 3.3 * 3.3 * 12.0)),
        },
        Case {
            level: "simple",
            name: "shell 2 inside, box open at the top",
            budget_ms: 300.0,
            build: s_shell_box,
            want: Want::Volume(shell_box),
        },
        Case {
            level: "simple",
            name: "shell 2 outside",
            budget_ms: 300.0,
            build: s_shell_outside,
            want: Want::Volume(44.0 * 34.0 * 22.0 - 24_000.0),
        },
        Case {
            level: "simple",
            name: "shell 2 both ways",
            budget_ms: 300.0,
            build: s_shell_both,
            want: Want::Volume(42.0 * 32.0 * 21.0 - 38.0 * 28.0 * 19.0),
        },
        Case {
            level: "simple",
            name: "shell 2 of a cylinder r15, open top",
            budget_ms: 300.0,
            build: s_shell_cylinder,
            want: Want::Volume(PI * (225.0 * 50.0 - 169.0 * 48.0)),
        },
        Case {
            level: "simple",
            name: "shell 2, no face removed: closed cavity",
            budget_ms: 300.0,
            build: s_shell_closed,
            want: Want::Volume(24_000.0 - 36.0 * 26.0 * 16.0),
        },
        Case {
            level: "complex",
            name: "fillet r5 then shell 2",
            budget_ms: 500.0,
            build: c_fillet_then_shell,
            want: Want::Volume(
                24_000.0 - round4(5.0, 20.0) - (36.0 * 26.0 - round4(3.0, 1.0)) * 18.0,
            ),
        },
        Case {
            level: "complex",
            name: "shell 2 then fillet r1 outside",
            budget_ms: 500.0,
            build: c_shell_then_fillet,
            want: Want::Volume(shell_box - round4(1.0, 20.0)),
        },
        Case {
            level: "complex",
            name: "shell 2 then chamfer 0.5 bottom",
            budget_ms: 500.0,
            build: c_shell_then_chamfer,
            want: Want::Volume(shell_box - (0.125 * 140.0 - 4.0 * 0.125 / 3.0)),
        },
        Case {
            level: "complex",
            name: "shell 2 then hole d6 through the front wall",
            budget_ms: 500.0,
            build: c_shell_then_hole,
            want: Want::Volume(shell_box - PI * 9.0 * 2.0),
        },
        Case {
            level: "complex",
            name: "shell 2 open at top and front",
            budget_ms: 300.0,
            build: c_shell_two_faces,
            want: Want::Volume(24_000.0 - 36.0 * 28.0 * 18.0),
        },
        Case {
            level: "complex",
            name: "shell edited 2 -> 3, fillet after it kept",
            budget_ms: 800.0,
            build: c_shell_edit,
            want: Want::Volume(24_000.0 - 34.0 * 24.0 * 17.0 - round4(1.0, 20.0)),
        },
        Case {
            level: "complex",
            name: "shell 2 after a -5° taper (SolidCraft #2178)",
            budget_ms: 500.0,
            build: c_shell_after_taper,
            want: Want::Volume(taper_shell_volume()),
        },
        Case {
            level: "complex",
            name: "shell 2 of a disc rounded r1, bottom open (#319)",
            budget_ms: 1500.0,
            build: c_shell_disc_small_round,
            want: Want::Volume({
                let c = 0.5 * (10.0 - 3.0 * PI) / (3.0 * (4.0 - PI)) * 2.0;
                let loss = 2.0 * PI * (20.0 - c) * (1.0 - PI / 4.0);
                PI * 400.0 * 5.0 - loss - PI * 324.0 * 3.0
            }),
        },
        Case {
            level: "complex",
            name: "shell 2, round r2 at the open top: refused (#342)",
            budget_ms: 500.0,
            build: c_shell_disc_round_as_thick,
            want: Want::Error("rim"),
        },
        Case {
            level: "complex",
            name: "shell 1, box + cylinder on its edge (#1494)",
            budget_ms: 1000.0,
            build: c_shell_box_with_edge_cylinder,
            want: Want::VolumeWithin(edge_cylinder_shell_volume(), 30.0),
        },
        Case {
            level: "simple",
            name: "draft 5° fixed plane XY, four sides",
            budget_ms: 300.0,
            build: s_draft_fixed_plane,
            want: Want::Volume(frustum_volume()),
        },
        Case {
            level: "simple",
            name: "draft 5° fixed top edges (lean out below)",
            budget_ms: 300.0,
            build: s_draft_fixed_top_edges,
            want: Want::Volume(simpson(
                &|z| (40.0 + 2.0 * (20.0 - z) * k5()) * (30.0 + 2.0 * (20.0 - z) * k5()),
                0.0,
                20.0,
            )),
        },
        Case {
            level: "simple",
            name: "draft 5° one face, fixed bottom edge",
            budget_ms: 300.0,
            build: s_draft_one_face,
            want: Want::Volume(40.0 * (600.0 - 200.0 * k5())),
        },
        Case {
            level: "simple",
            name: "draft 5° of a cylinder wall r15 (#547)",
            budget_ms: 300.0,
            build: s_draft_cylinder,
            want: Want::Volume({
                let r = 15.0 - 50.0 * k5();
                PI * 50.0 / 3.0 * (225.0 + 15.0 * r + r * r)
            }),
        },
        Case {
            level: "complex",
            name: "draft: front at its bottom, right at its top",
            budget_ms: 400.0,
            build: c_draft_two_fixed_edges,
            want: Want::Volume(24_000.0 - 8000.0 * k5() + k5() * (6000.0 - k5() * 4000.0 / 3.0)),
        },
        Case {
            level: "complex",
            name: "fillet r5 then draft 5° of all sides",
            budget_ms: 500.0,
            build: c_fillet_then_draft,
            want: Want::Volume(rounded_frustum_volume()),
        },
        Case {
            level: "complex",
            name: "draft 5° then fillet r1 round the top",
            budget_ms: 500.0,
            build: c_draft_then_fillet,
            want: Want::VolumeWithin(draft_fillet_volume(), 1.0),
        },
        Case {
            level: "complex",
            name: "draft 5° then shell 2",
            budget_ms: 500.0,
            build: c_draft_then_shell,
            want: Want::Volume(taper_shell_volume()),
        },
        Case {
            level: "complex",
            name: "shell 2 then draft 5° of the outside",
            budget_ms: 500.0,
            build: c_shell_then_draft,
            want: Want::Volume(frustum_volume() - 36.0 * 26.0 * 18.0),
        },
        Case {
            level: "complex",
            name: "draft edited 5° -> 3°, fillet after it kept",
            budget_ms: 800.0,
            build: c_draft_edit,
            want: Want::VolumeWithin(
                {
                    let k = 3f64.to_radians().tan();
                    let phi = 93f64.to_radians();
                    let lost = 1.0 / (phi / 2.0).tan() - (PI - phi) / 2.0;
                    let v = simpson(&|z| (40.0 - 2.0 * z * k) * (30.0 - 2.0 * z * k), 0.0, 20.0);
                    v - lost * (140.0 - 160.0 * k)
                },
                1.0,
            ),
        },
        Case {
            level: "complex",
            name: "draft 95°: refused",
            budget_ms: 300.0,
            build: c_draft_bad_angle,
            want: Want::Error("between"),
        },
        Case {
            level: "complex",
            name: "all edges r2, shell 2, top removed (round = wall)",
            budget_ms: 1500.0,
            build: c_rounded_box_shell_r2,
            want: Want::Volume(rounded_box_shell_volume(2.0)),
        },
        Case {
            level: "complex",
            name: "all edges r3, shell 2, top removed (round > wall)",
            budget_ms: 2500.0,
            build: c_rounded_box_shell_r3,
            want: Want::Volume(rounded_box_shell_volume(3.0)),
        },
        Case {
            level: "complex",
            name: "shell 16 in a box 20 high: refused",
            budget_ms: 300.0,
            build: c_shell_too_thick,
            want: Want::Error("geometry kernel"),
        },
        Case {
            level: "complex",
            name: "pocket: 2 circles + line + arc, 8 deep",
            budget_ms: 100.0,
            build: c_pocket,
            want: Want::VolumeWithin(pocket_volume(false, false, false, false), 8.0),
        },
        Case {
            level: "complex",
            name: "pocket rim rounded r2",
            budget_ms: 300.0,
            build: c_pocket_rim,
            want: Want::VolumeWithin(pocket_volume(false, true, false, false), 8.0),
        },
        Case {
            level: "complex",
            name: "pocket floor rounded r2",
            budget_ms: 300.0,
            build: c_pocket_floor,
            want: Want::VolumeWithin(pocket_volume(false, false, true, false), 8.0),
        },
        Case {
            level: "complex",
            name: "pocket rim chamfered 1.5",
            budget_ms: 300.0,
            build: c_pocket_rim_chamfer,
            want: Want::VolumeWithin(pocket_volume(false, true, false, true), 8.0),
        },
        Case {
            level: "very complex",
            name: "pocket rim and floor rounded r2",
            budget_ms: 500.0,
            build: v_pocket_rim_and_floor,
            want: Want::VolumeWithin(pocket_volume(false, true, true, false), 8.0),
        },
        Case {
            level: "very complex",
            name: "pocket with an inward arc, rim r2",
            budget_ms: 500.0,
            build: v_dent_pocket_rim,
            want: Want::VolumeWithin(pocket_volume(true, true, false, false), 8.0),
        },
        Case {
            level: "very complex",
            name: "pocket with an inward arc, floor r2",
            budget_ms: 500.0,
            build: v_dent_pocket_floor,
            want: Want::VolumeWithin(pocket_volume(true, false, true, false), 8.0),
        },
        Case {
            level: "very complex",
            name: "pocket: rim r2 and floor r1 (two edge sets)",
            budget_ms: 500.0,
            build: v_pocket_two_sets,
            want: Want::VolumeWithin(two_sets_volume(), 8.0),
        },
        Case {
            level: "complex",
            name: "pocket floor chamfered 1 (concave)",
            budget_ms: 300.0,
            build: c_pocket_floor_chamfer,
            want: Want::VolumeWithin(
                {
                    let (a, l) = pocket_area_perimeter(false);
                    100.0 * 60.0 * 12.0 - a * 8.0 + 0.5 * (l - 2.0 * PI / 3.0)
                },
                8.0,
            ),
        },
        Case {
            level: "very complex",
            name: "Solid > All Rounds r1 (box edges + pocket rim)",
            budget_ms: 1500.0,
            build: v_all_rounds,
            want: Want::VolumeWithin(
                {
                    let (a, l) = pocket_area_perimeter(false);
                    let r: Real = 1.0;
                    let d = r * (10.0 - 3.0 * PI) / (3.0 * (4.0 - PI));
                    100.0 * 60.0 * 12.0
                        - a * 8.0
                        - rounded_box_loss(100.0, 60.0, 12.0, r)
                        - (1.0 - PI / 4.0) * r * r * (l + 2.0 * PI * d)
                },
                8.0,
            ),
        },
        Case {
            level: "complex",
            name: "variable radius 2..5 on a box edge",
            budget_ms: 300.0,
            build: c_variable_fillet,
            // Linear r: ∫(1 − π/4) r² dx = (1 − π/4) L (r0² + r0 r1 + r1²) / 3.
            want: Want::VolumeWithin(
                24000.0 - (1.0 - PI / 4.0) * 40.0 * (4.0 + 10.0 + 25.0) / 3.0,
                1.5,
            ),
        },
        Case {
            level: "complex",
            name: "partial chamfer 2, 15 of 40 mm",
            budget_ms: 300.0,
            build: c_partial_chamfer,
            want: Want::VolumeWithin(24000.0 - 2.0 * 15.0, 0.01),
        },
        Case {
            level: "complex",
            name: "partial chamfer past the edge's end",
            budget_ms: 300.0,
            build: c_partial_chamfer_too_long,
            want: Want::Error("runs past the end of the edge"),
        },
        Case {
            level: "complex",
            name: "chord width 4 on a box edge",
            budget_ms: 300.0,
            build: c_chord_fillet,
            want: Want::VolumeWithin(24000.0 - (1.0 - PI / 4.0) * 8.0 * 40.0, 1.5),
        },
        Case {
            level: "complex",
            name: "Full Round of a 6 mm rib end",
            budget_ms: 300.0,
            build: c_full_round,
            want: Want::VolumeWithin(840.0 - (4.0 - PI) * 9.0 * 5.0, 1.0),
        },
        Case {
            level: "simple",
            name: "box 40×30×20",
            budget_ms: 50.0,
            build: s_box,
            want: Want::Volume(24000.0),
        },
        Case {
            level: "simple",
            name: "cylinder r15 h50",
            budget_ms: 50.0,
            build: s_cylinder,
            want: Want::Volume(PI * 225.0 * 50.0),
        },
        Case {
            level: "simple",
            name: "revolved tube 10..20 × 20",
            budget_ms: 80.0,
            build: s_revolve_tube,
            want: Want::Volume(PI * (400.0 - 100.0) * 20.0),
        },
        Case {
            level: "simple",
            name: "fillet r5 on one edge",
            budget_ms: 80.0,
            build: s_fillet_edge,
            want: Want::Volume(24000.0 - (1.0 - PI / 4.0) * 25.0 * 20.0),
        },
        Case {
            level: "simple",
            name: "through hole Ø10",
            budget_ms: 100.0,
            build: s_through_hole,
            want: Want::Volume(40.0 * 30.0 * 10.0 - PI * 25.0 * 10.0),
        },
        Case {
            level: "simple",
            name: "extrude symmetric 20",
            budget_ms: 50.0,
            build: s_symmetric,
            want: Want::Volume(24000.0),
        },
        Case {
            level: "simple",
            name: "extrude asymmetric 15 / 5",
            budget_ms: 50.0,
            build: s_asymmetric,
            want: Want::Volume(24000.0),
        },
        Case {
            level: "simple",
            name: "extrude taper +5°",
            budget_ms: 80.0,
            build: s_taper,
            want: Want::Volume({
                let t = 5f64.to_radians().tan();
                1200.0 * 20.0 + 70.0 * t * 400.0 + 4.0 * t * t * 8000.0 / 3.0
            }),
        },
        Case {
            level: "simple",
            name: "revolve 270° (tube section)",
            budget_ms: 80.0,
            build: s_revolve_270,
            want: Want::Volume(0.75 * PI * 300.0 * 20.0),
        },
        Case {
            level: "simple",
            name: "chamfer two distances 2 × 4",
            budget_ms: 80.0,
            build: s_chamfer_two,
            want: Want::Volume(24000.0 - 4.0 * 40.0),
        },
        Case {
            level: "complex",
            name: "plate + slab (new solid, from face, cut)",
            budget_ms: 300.0,
            build: plate_and_slab,
            want: Want::Volume(12000.0 + 1200.0 * 5.0),
        },
        Case {
            level: "complex",
            name: "post: extrude To the slab's face",
            budget_ms: 300.0,
            build: c_extrude_to_face,
            want: Want::Volume(12000.0 + 6000.0 + PI * 25.0 * 10.0),
        },
        Case {
            level: "complex",
            name: "post: extrude To Next",
            budget_ms: 300.0,
            build: c_extrude_to_next,
            want: Want::Volume(12000.0 + 6000.0 + PI * 25.0 * 10.0),
        },
        Case {
            level: "complex",
            name: "intersect: cylinder r15 within the box",
            budget_ms: 200.0,
            build: c_intersect,
            want: Want::Volume(PI * 225.0 * 20.0),
        },
        Case {
            level: "complex",
            name: "boss r8 joined, base rounded r2 (concave)",
            budget_ms: 400.0,
            build: c_boss_base_fillet,
            want: Want::Volume(
                40.0 * 40.0 * 5.0
                    + PI * 64.0 * 10.0
                    + 2.0
                        * PI
                        * (8.0 + 2.0 * (10.0 - 3.0 * PI) / (3.0 * (4.0 - PI)))
                        * (1.0 - PI / 4.0)
                        * 4.0,
            ),
        },
        Case {
            level: "complex",
            name: "radial grille, 4 of 8 sectors cut",
            budget_ms: 400.0,
            build: c_radial_grille,
            want: Want::Volume((PI * 400.0 - PI * 225.0 / 2.0) * 5.0),
        },
        Case {
            level: "complex",
            name: "checkerboard grille 6×6, 2 mm webs",
            budget_ms: 600.0,
            build: c_checker_grille,
            want: Want::Volume((3600.0 - 18.0 * 64.0) * 3.0),
        },
        Case {
            level: "complex",
            name: "checkerboard touching at corners",
            budget_ms: 3000.0,
            build: c_checker_touching,
            want: Want::Volume((1600.0 - 800.0) * 3.0),
        },
        Case {
            level: "complex",
            name: "cut pointing away from the body",
            budget_ms: 100.0,
            build: c_cut_away,
            want: Want::Error("does not intersect"),
        },
        Case {
            level: "complex",
            name: "edit: through hole → 4 deep, fillet after",
            budget_ms: 300.0,
            build: c_edit_feature,
            want: Want::Volume(
                40.0 * 30.0 * 10.0 - PI * 25.0 * 4.0 - (1.0 - PI / 4.0) * 4.0 * 40.0,
            ),
        },
        Case {
            level: "very complex",
            name: "checkerboard grille 12×12 (72 holes)",
            budget_ms: 3000.0,
            build: v_checker_grille_12,
            want: Want::Volume((96.0 * 96.0 - 72.0 * 36.0) * 3.0),
        },
        Case {
            level: "very complex",
            name: "radial grille, 12 of 24 sectors cut",
            budget_ms: 1500.0,
            build: v_radial_grille_24,
            want: Want::Volume((PI * 1600.0 - PI * 34.0 * 34.0 / 2.0) * 5.0),
        },
        Case {
            level: "very complex",
            name: "36 holes Ø4, top edges chamfered 0.5",
            budget_ms: 5000.0,
            build: v_holes_chamfered,
            want: Want::Volume(60.0 * 60.0 * 6.0 - 36.0 * (PI * 4.0 * 6.0 + ring)),
        },
        Case {
            level: "very complex",
            name: "36 holes Ø4, top edges rounded r0.5",
            budget_ms: 5000.0,
            build: v_holes_filleted,
            want: Want::Volume(60.0 * 60.0 * 6.0 - 36.0 * (PI * 4.0 * 6.0 + round)),
        },
    ]
}

#[test]
fn feature_matrix() {
    let mut problems = Vec::new();
    println!(
        "{:<13} {:<44} {:>8} {:>9}  result",
        "level", "case", "budget", "time"
    );
    for c in cases() {
        let t0 = Instant::now();
        let built = (c.build)();
        let mut ms = t0.elapsed().as_secs_f64() * 1000.0;
        // Over budget: once more, the faster run counts (a busy machine
        // must not fail a case; slow code stays slow in both runs).
        if ms > c.budget_ms {
            let t1 = Instant::now();
            let _ = (c.build)();
            ms = ms.min(t1.elapsed().as_secs_f64() * 1000.0);
        }
        let verdict = match (&built, &c.want) {
            (Ok(part), Want::Volume(_) | Want::VolumeWithin(..)) => {
                let (want, tol) = match c.want {
                    Want::VolumeWithin(w, t) => (w, t),
                    Want::Volume(w) => (w, 2e-3 * w),
                    Want::Error(_) => (0.0, 0.0),
                };
                let v = volume(part);
                let mut bad = Vec::new();
                if !part.exact {
                    bad.push("faceted kernel".to_owned());
                }
                for (sid, _) in part.body.solids() {
                    if let Err(e) = part.body.validate_solid(sid) {
                        bad.push(format!("invalid: {e}"));
                    }
                }
                if !part.feature_health.is_empty() {
                    bad.push(format!("sick: {:?}", part.feature_health));
                }
                if (v - want).abs() > tol {
                    bad.push(format!("volume {v:.2} vs {want:.2}"));
                }
                if bad.is_empty() {
                    Ok(format!("V {v:.1}"))
                } else {
                    Err(bad.join("; "))
                }
            }
            (Err(e), Want::Error(text)) if e.contains(text) => Ok(format!("refused: {e}")),
            (Ok(_), Want::Error(text)) => Err(format!("built, but should be refused ({text})")),
            (Err(e), _) => Err(format!("failed: {e}")),
        };
        let over = ms > c.budget_ms;
        let line = match &verdict {
            Ok(v) => v.clone(),
            Err(e) => format!("FAIL {e}"),
        };
        println!(
            "{:<13} {:<44} {:>6.0} ms {:>7.0} ms{}  {line}",
            c.level,
            c.name,
            c.budget_ms,
            ms,
            if over { " OVER" } else { "" }
        );
        if let Err(e) = verdict {
            problems.push(format!("{} / {}: {e}", c.level, c.name));
        }
        if over {
            problems.push(format!(
                "{} / {}: {ms:.0} ms > {:.0} ms",
                c.level, c.name, c.budget_ms
            ));
        }
    }
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
}
