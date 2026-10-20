//! Timing probe of large feature patterns (`docs/PERF.md`): a plate
//! 5n × 5n × 3 with one through hole r1 at (2.5, 2.5) patterned n × n at
//! 5 mm, the same with a joined boss, and a circular pattern of holes on a
//! disc. Prints the time of `add_pattern` and of the preview's edge
//! polylines, and checks the volume analytically.
//!
//! ```sh
//! cargo run --release --no-default-features --example pattern_probe -- hole 10 20 30 50 71
//! cargo run --release --no-default-features --example pattern_probe -- boss 30
//! cargo run --release --no-default-features --example pattern_probe -- circ 1000
//! ```

use std::f64::consts::PI;
use std::time::Instant;

use cadcraft3d::kernel::{Point3, Real, TessellationOptions};
use cadcraft3d::part::{
    Extent, ExtentDirection, ExtrudeFeature, LineRef, Operation, OriginPlane, PartDocument,
    PatternDirection, PatternFeature, PatternKind, ProfileRef, PullRef, SketchOptions,
    SketchPlacement,
};
use cadcraft3d::sketch::Point2;

fn opts() -> SketchOptions {
    SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    }
}

/// A circle r at (x, y) on the top face (z = h) of `part`: a through hole
/// (`cut`) or a boss `boss` high.
fn circle_feature(
    part: &mut PartDocument,
    top: &str,
    h: Real,
    x: Real,
    y: Real,
    r: Real,
    boss: Option<Real>,
) {
    let j = part
        .create_sketch(SketchPlacement::on_face(top), &opts())
        .unwrap();
    let c = part.sketches[j].sketch.plane.project(&Point3::new(x, y, h));
    part.sketches[j].sketch.add_circle(c, r).unwrap();
    let rg = part.sketches[j].sketch.regions();
    let mut ex = ExtrudeFeature::new(
        vec![ProfileRef::of(&rg[0], rg[0].interior_point())],
        boss.unwrap_or(1.0),
    );
    match boss {
        None => {
            ex.operation = Operation::Cut;
            ex.extent = Extent::ThroughAll;
            ex.direction = ExtentDirection::Flipped;
        }
        Some(_) => ex.operation = Operation::Join,
    }
    part.add_extrude(j, ex).unwrap();
}

fn grid(n: u32, s: Real) -> PatternFeature {
    let mut p = PatternFeature::rectangular();
    p.features = vec!["Extrusion1".into()];
    if let PatternKind::Rectangular { first, second } = &mut p.kind {
        (first.dir, first.count, first.spacing) = (Some(PullRef::Line(LineRef::Origin(0))), n, s);
        *second = Some(PatternDirection {
            dir: Some(PullRef::Line(LineRef::Origin(1))),
            count: n,
            spacing: s,
            ..PatternDirection::new()
        });
    }
    p
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let kind = args.first().cloned().unwrap_or_else(|| "hole".into());
    let sizes: Vec<u32> = args[1.min(args.len())..]
        .iter()
        .filter_map(|s| s.parse().ok())
        .collect();
    for n in sizes {
        let t0 = Instant::now();
        let (mut part, def, expect, count) = match kind.as_str() {
            "hole" | "boss" => {
                let side = 5.0 * n as Real;
                let mut part = PartDocument::with_box("Box1", [side, side, 3.0]).unwrap();
                let boss = (kind == "boss").then_some(2.0);
                circle_feature(&mut part, "Box1:Top", 3.0, 2.5, 2.5, 1.0, boss);
                let k = (n * n) as Real;
                let v = side * side * 3.0
                    + match boss {
                        None => -k * PI * 3.0,
                        Some(h) => k * PI * h,
                    };
                (part, grid(n, 5.0), v, n * n)
            }
            "circ" => {
                // n holes r0.4 on a ring (1.2 mm apart, radius at least 180) on a disc 10 wider, 3 thick.
                let ring = (n as Real * 1.2 / (2.0 * PI)).max(180.0);
                let disc = ring + 10.0;
                let mut part = PartDocument::default();
                let i = part
                    .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
                    .unwrap();
                part.sketches[i]
                    .sketch
                    .add_circle(Point2::new(0.0, 0.0), disc)
                    .unwrap();
                let rg = part.sketches[i].sketch.regions();
                part.add_extrude(
                    i,
                    ExtrudeFeature::new(vec![ProfileRef::of(&rg[0], rg[0].interior_point())], 3.0),
                )
                .unwrap();
                circle_feature(&mut part, "Extrusion1:End", 3.0, ring, 0.0, 0.4, None);
                let mut p = PatternFeature::circular();
                p.features = vec!["Extrusion2".into()];
                if let PatternKind::Circular { axis, count, .. } = &mut p.kind {
                    (*axis, *count) = (Some(LineRef::Origin(2)), n);
                }
                let v = PI * disc * disc * 3.0 - n as Real * PI * 0.16 * 3.0;
                (part, p, v, n)
            }
            _ => panic!("kind: hole | boss | circ"),
        };
        let setup = t0.elapsed().as_secs_f64();
        part.build_cache = Default::default();
        part.regenerate().unwrap();
        let t1 = Instant::now();
        let id = part.add_pattern(def).unwrap();
        let add = t1.elapsed().as_secs_f64();
        let t2 = Instant::now();
        let o = TessellationOptions::default();
        let lines: usize = part
            .body
            .feature_edges(&id)
            .into_iter()
            .filter_map(|e| part.body.sample_edge(e, &o).ok())
            .count();
        let poly = t2.elapsed().as_secs_f64();
        let t3 = Instant::now();
        let v: Real = part
            .exact_bodies
            .0
            .iter()
            .map(|b| b.measure_full().map(|m| m.volume).unwrap_or(0.0))
            .sum();
        let meas = t3.elapsed().as_secs_f64();
        let faces = part.body.faces().count();
        let named = part
            .body
            .faces()
            .filter(|(_, f)| {
                f.name
                    .as_deref()
                    .is_some_and(|s| s.starts_with(&format!("{id}#")))
            })
            .count();
        println!(
            "{kind} {count:>6}: add_pattern {add:7.2} s, polylines {poly:5.2} s ({lines} edges), setup {setup:5.2} s, \
             exact {}, sick {:?}, faces {faces}, named copies {named}, volume {v:.3} vs {expect:.3} (rel {:.1e}), measure {meas:.1} s",
            part.exact,
            part.feature_health,
            ((v - expect) / expect).abs(),
        );
    }
}
