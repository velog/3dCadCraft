//! A plate with a pocket shaped by two circles joined by a line on one side
//! and a tangent arc on the other, its rim and floor rounded (the same part
//! as the `tests/matrix.rs` pocket cases).
//!
//! ```sh
//! cargo run --example pocket -- /tmp/pocket.3dcadcraft          # arc bulging out
//! cargo run --example pocket -- /tmp/dent.3dcadcraft dent       # arc curving in
//! cargo run --example pocket -- /tmp/pocket0.3dcadcraft plain   # no rounds (fillet in the GUI)
//! ```

use std::f64::consts::PI;
use std::path::PathBuf;

use cadcraft3d::document::{DocumentData, Project};
use cadcraft3d::kernel::{Point3, Real};
use cadcraft3d::part::blend::edge_ref;
use cadcraft3d::part::{
    BlendFeature, EdgeRef, ExtentDirection, ExtrudeFeature, Operation, OriginPlane, PartDocument,
    ProfileRef, SketchOptions, SketchPlacement,
};
use cadcraft3d::sketch::Point2;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path: PathBuf = std::env::args_os()
        .nth(1)
        .map_or("pocket.3dcadcraft".into(), Into::into);
    let arg = std::env::args().nth(2).unwrap_or_default();
    let mut part = pocket_plate(arg == "dent")?;
    if arg != "plain" {
        let in_pocket = |h: [Real; 3], z: Real| {
            (h[2] - z).abs() < 1e-6 && h[0] > 10.0 && h[0] < 90.0 && h[1] > 5.0 && h[1] < 58.0
        };
        let e: Vec<EdgeRef> = edges_where(&part, |h| in_pocket(h, 12.0) || in_pocket(h, 4.0));
        println!("{} pocket edges", e.len());
        part.add_blend(BlendFeature::fillet(e, 2.0))?;
    }
    println!(
        "exact: {}, faces: {}",
        part.exact,
        part.body.faces().count()
    );
    let mut project = Project::new("Pocket");
    project.add_document("Part1", DocumentData::Part(part));
    project.save(&path)?;
    println!("saved {}", path.display());
    Ok(())
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

fn edges_where(part: &PartDocument, keep: impl Fn([Real; 3]) -> bool) -> Vec<EdgeRef> {
    part.body
        .edges()
        .filter_map(|(e, _)| edge_ref(&part.body, e))
        .filter(|r| keep(r.hint))
        .collect()
}

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
