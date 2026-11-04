//! The tool of a profile wrapped onto a cylinder (`docs/spec/2.17-emboss.md`).

#[allow(unused_imports)]
use super::compat::prelude::*;
use super::*;

/// A ring sector about the line through `origin` along `axis`: radii `r0..r1`, heights `z0..z1`
/// along the axis, `angle` radians centred on the direction `radial` (square to the axis).
pub fn ring_sector(
    origin: Point3,
    axis: Vector3,
    radial: Vector3,
    (r0, r1): (Real, Real),
    (z0, z1): (Real, Real),
    angle: Real,
    prefix: &str,
) -> Result<ExactSolid, String> {
    if !(r0 > 0.0 && r1 > r0 && z1 > z0) || !(angle > 0.0 && angle < std::f64::consts::TAU) {
        return Err("the wrapped profile does not fit the cylinder".into());
    }
    let (Some(a), Some(x)) = (
        nalgebra::Unit::try_new(axis, 1e-12),
        nalgebra::Unit::try_new(radial, 1e-12),
    ) else {
        return Err("the cylinder has no axis".into());
    };
    let plane = Plane {
        origin,
        normal: nalgebra::Unit::new_normalize(x.cross(&a)),
        x_axis: x,
    };
    let pts = [(r0, z0), (r1, z0), (r1, z1), (r0, z1)].map(|(u, v)| nalgebra::Point2::new(u, v));
    let segs = (0..4)
        .map(|k| sg::Seg2::Line {
            a: v2(&pts[k]),
            b: v2(&pts[(k + 1) % 4]),
        })
        .collect();
    let prof = ExactProfile::from_region(sg::Region2 {
        outer: sg::Loop2 { segs },
        holes: Vec::new(),
    });
    // Half each way from the plane, whichever way the kernel turns.
    revolve(
        &plane,
        &prof,
        nalgebra::Point2::origin(),
        nalgebra::Vector2::y(),
        -angle / 2.0,
        angle,
        prefix,
    )
}
