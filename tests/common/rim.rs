//! Teeth cut into the blended rim of a revolved disc (`tests/rim_teeth.rs`, `tests/perf.rs`).

use super::*;
use cadcraft3d::part::{LineRef, PatternFeature, PatternKind};

/// The disc's radius and height.
pub const R: Real = 30.0;
pub const H: Real = 10.0;

/// A disc R × H revolved about Z from a rectangle on XZ: its seam lies in the XZ plane.
pub fn disc() -> PartDocument {
    let mut p = PartDocument::default();
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XZ), &opts())
        .unwrap();
    p.sketches[i]
        .sketch
        .add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(R, H))
        .unwrap();
    let r = p.sketches[i].sketch.regions();
    p.add_revolve(
        i,
        RevolveFeature::new(region_profiles(&r, |_| true), AxisRef::Origin(2)),
    )
    .unwrap();
    p
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Blend {
    Chamfer,
    Fillet,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Teeth {
    /// `n` rectangles in one sketch.
    Sketch(usize),
    /// One rectangle and a circular pattern of `n`.
    Pattern(usize),
}

#[derive(Clone, Copy, Debug)]
pub struct Case {
    pub blend: Blend,
    /// The chamfer's distance or the fillet's radius on the top rim.
    pub size: Real,
    /// How far the teeth reach in from the outer radius.
    pub depth: Real,
    /// Cut from the blended (top) face, else from the bottom.
    pub from_top: bool,
    /// Through All, else 5 mm.
    pub through: bool,
    pub teeth: Teeth,
}

/// Tooth `k` of `n` (the first on the X axis, the revolve's seam): 4 mm wide, from
/// `R - depth` to past the rim.
fn tooth(
    sk: &mut cadcraft3d::sketch::Sketch,
    at: &dyn Fn(Real, Real, Real) -> Point2,
    z: Real,
    k: usize,
    n: usize,
    depth: Real,
) {
    let a = std::f64::consts::TAU * k as Real / n as Real;
    let (c, s) = (a.cos(), a.sin());
    let p = |r: Real, t: Real| at(r * c - t * s, r * s + t * c, z);
    let (r0, r1) = (R - depth, R + 3.0);
    sk.add_rectangle_three_point(p(r0, -2.0), p(r1, -2.0), p(r1, 2.0))
        .unwrap();
}

/// The part of `c`, or the first step's refusal.
pub fn build(c: Case) -> Result<PartDocument, String> {
    let mut p = disc();
    let rim = edges_where(&p, |h| {
        (h[2] - H).abs() < 1e-6 && ((h[0] * h[0] + h[1] * h[1]).sqrt() - R).abs() < 1e-3
    });
    if rim.is_empty() {
        return Err("no rim edge".into());
    }
    let def = match c.blend {
        Blend::Chamfer => BlendFeature::chamfer(rim, c.size),
        Blend::Fillet => BlendFeature::fillet(rim, c.size),
    };
    p.add_blend(def).map_err(|e| format!("blend: {e}"))?;
    let z = if c.from_top { H } else { 0.0 };
    let face = face_where(&p, |q| (q.z - z).abs() < 1e-6);
    let n = match c.teeth {
        Teeth::Sketch(n) => n,
        Teeth::Pattern(_) => 1,
    };
    let j = sketch_on(&mut p, &face, |sk, at| {
        for k in 0..n {
            tooth(sk, at, z, k, n, c.depth);
        }
    });
    // The teeth only (the projected outline of the face makes regions of its own).
    let regions = p.sketches[j].sketch.regions();
    let small = 4.0 * (c.depth + 3.0) + 1.0;
    let mut ex = ExtrudeFeature::new(
        region_profiles(&regions, |r| r.holes.is_empty() && r.area() < small),
        5.0,
    );
    ex.operation = Operation::Cut;
    ex.direction = ExtentDirection::Flipped;
    if c.through {
        ex.extent = Extent::ThroughAll;
    }
    let id = p.add_extrude(j, ex).map_err(|e| format!("cut: {e}"))?;
    if let Teeth::Pattern(n) = c.teeth {
        let mut pat = PatternFeature::circular();
        pat.features = vec![id];
        if let PatternKind::Circular { axis, count, .. } = &mut pat.kind {
            (*axis, *count) = (Some(LineRef::Origin(2)), n as u32);
        }
        p.add_pattern(pat).map_err(|e| format!("pattern: {e}"))?;
    }
    Ok(p)
}
