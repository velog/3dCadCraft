//! Chamfers of the rims of a turned shaft with a flange (the owner's session of 2026-11-13,
//! `tests/data/revolve_flange_chamfer_journal.jsonl`: a profile revolved about X, the flange
//! r = 24 from x = 41 to 57, its right face running into a 45° cone). SMLib refused the right
//! rim (status 1001) while it took the left one; every rim must now be chamfered, exactly.
//!
//! An equal chamfer `d` of a whole circle of radius `R` between a plane square to the axis and a
//! coaxial cylinder takes (or adds) a ring whose section is the right triangle with legs `d`:
//! area d²/2, its centroid d/3 in from the corner, so by Pappus the ring is
//! `2π (R ∓ d/3) · d²/2 = π d² (R ∓ d/3)` (minus for an outer rim, cut; plus for an inner corner,
//! filled, its centroid outside the corner's radius).

mod common;
use common::*;

use cadcraft3d::diagnostics::replay;
use cadcraft3d::part::blend::{BlendFeature, EdgeRef};
use cadcraft3d::part::{AxisRef, OriginPlane, PartDocument, RevolveFeature, SketchPlacement};
use std::f64::consts::PI;
use std::path::Path;

const JOURNAL: &str = "tests/data/revolve_flange_chamfer_journal.jsonl";

/// The part right after the owner's Revolve (state 11).
fn shaft() -> PartDocument {
    replay::part_at(Path::new(JOURNAL), 11).unwrap_or_else(|e| panic!("{e}"))
}

fn fine(p: &PartDocument) -> f64 {
    p.exact_bodies.0[0]
        .volume_at(1e-3)
        .unwrap_or_else(|| panic!("no volume"))
}

/// The circle at axial position `x` and radius `r` (the shaft's axis is X).
fn rim(p: &PartDocument, x: f64, r: f64) -> Vec<EdgeRef> {
    let e = edges_where(p, |h| {
        (h[0] - x).abs() < 1e-6 && ((h[1] * h[1] + h[2] * h[2]).sqrt() - r).abs() < 1e-6
    });
    assert!(!e.is_empty(), "no edge at x = {x}, r = {r}");
    e
}

/// What an equal chamfer `d` of an outer rim of radius `r` cuts off.
fn outer(r: f64, d: f64) -> f64 {
    PI * d * d * (r - d / 3.0)
}

/// What it fills at an inner corner of radius `r`.
fn inner(r: f64, d: f64) -> f64 {
    PI * d * d * (r + d / 3.0)
}

fn chamfered(p: &PartDocument, edges: Vec<EdgeRef>, d: f64) -> PartDocument {
    let mut q = p.clone();
    q.add_blend(BlendFeature::chamfer(edges, d))
        .unwrap_or_else(|e| panic!("chamfer {d}: {e}"));
    assert!(
        q.exact && q.feature_health.is_empty(),
        "{:?}",
        q.feature_health
    );
    q
}

fn close(got: f64, want: f64, what: &str) {
    assert!((got - want).abs() < 1e-3 * want, "{what}: {got} vs {want}");
}

#[test]
fn both_flange_rims_chamfer_alone_at_every_size() {
    let p = shaft();
    let before = fine(&p);
    for x in [41.0, 57.0] {
        for d in [0.5, 1.0, 2.0, 3.0] {
            let q = chamfered(&p, rim(&p, x, 24.0), d);
            close(
                before - fine(&q),
                outer(24.0, d),
                &format!("rim x = {x}, d = {d}"),
            );
            // The new face is named after the edge it replaces.
            assert!(
                q.body
                    .faces()
                    .filter_map(|(_, f)| f.name.as_deref())
                    .any(|n| n.contains(":Bevel[")),
                "no bevel face at x = {x}, d = {d}"
            );
        }
    }
}

#[test]
fn the_flange_rims_and_its_inner_corner_together() {
    let p = shaft();
    let before = fine(&p);
    let d = 2.0;
    let mut edges = rim(&p, 41.0, 24.0);
    edges.extend(rim(&p, 57.0, 24.0));
    let q = chamfered(&p, edges.clone(), d);
    close(before - fine(&q), 2.0 * outer(24.0, d), "both rims");
    // With the corner where the shaft (r = 10) meets the flange: filled.
    edges.extend(rim(&p, 41.0, 10.0));
    let q = chamfered(&p, edges, d);
    close(
        before - fine(&q),
        2.0 * outer(24.0, d) - inner(10.0, d),
        "both rims and the corner",
    );
    // The same part again from its features (a reopened file).
    let mut again = q.clone();
    again.build_cache = Default::default();
    again.regenerate().unwrap_or_else(|e| panic!("{e}"));
    close(fine(&again), fine(&q), "regenerated");
}

/// A profile in the XZ plane (x = radius, z = height) revolved about Z.
fn turned(pts: &[(f64, f64)]) -> PartDocument {
    let mut p = PartDocument::default();
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XZ), &opts())
        .unwrap_or_else(|e| panic!("{e}"));
    let sk = &mut p.sketches[i].sketch;
    let ids: Vec<_> = pts
        .iter()
        .map(|&(x, y)| sk.add_point(Point2::new(x, y)))
        .collect();
    for k in 0..ids.len() {
        sk.add_line_between(ids[k], ids[(k + 1) % ids.len()])
            .unwrap_or_else(|e| panic!("{e}"));
    }
    let reg = sk.regions();
    p.add_revolve(
        i,
        RevolveFeature::new(region_profiles(&reg, |_| true), AxisRef::Origin(2)),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    p
}

/// The same flange drawn with exact numbers: its face running on into a cone, and into a step.
#[test]
fn flange_rims_of_drawn_profiles() {
    let cone = [
        (0.0, 0.0),
        (10.0, 0.0),
        (10.0, 41.0),
        (24.0, 41.0),
        (24.0, 57.0),
        (16.0, 57.0),
        (8.0, 65.0),
        (8.0, 100.0),
        (0.0, 100.0),
    ];
    let step = [
        (0.0, 0.0),
        (10.0, 0.0),
        (10.0, 41.0),
        (24.0, 41.0),
        (24.0, 57.0),
        (16.0, 57.0),
        (16.0, 65.0),
        (8.0, 65.0),
        (8.0, 100.0),
        (0.0, 100.0),
    ];
    for prof in [&cone[..], &step[..]] {
        let p = turned(prof);
        let before = fine(&p);
        for z in [41.0, 57.0] {
            let edges = edges_where(&p, |h| {
                ((h[0] * h[0] + h[1] * h[1]).sqrt() - 24.0).abs() < 1e-6 && (h[2] - z).abs() < 1e-6
            });
            let q = chamfered(&p, edges, 2.0);
            close(before - fine(&q), outer(24.0, 2.0), &format!("z = {z}"));
        }
    }
}
