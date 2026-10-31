//! Contour Roll (spec 5.6 §6.1j): an open profile of lines, thickened and
//! bent at its corners as a Contour Flange is, revolved about an axis of its
//! sketch. The flat pattern is that Contour Flange's, as wide as the rolled
//! angle times the neutral radius (Inventor's Unrolled Length), so the
//! profile's bends stay bend lines of the flat.

use solidcraft_geom::{Region2, Vec2};

use super::recognize::{plane_of as face_plane, region_of};
use super::{ExactSolid, RuleValues, SheetModel, contour};
use crate::kernel::{Plane, Real};

/// How the neutral radius of a Contour Roll is found (Inventor's Unroll Method).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RollUnroll {
    /// The cylinder through the section's centroid.
    Centroid,
    /// A sketch line (a, b) parallel to the axis gives the neutral cylinder.
    Custom([Real; 2], [Real; 2]),
    /// The unrolled length itself.
    Length(Real),
    /// The neutral radius itself.
    Radius(Real),
    /// Inner radius + K · thickness (a profile of one line).
    KFactor(Real),
}

/// A Contour Roll: its solid, its sheet model (the flat's), the neutral
/// radius and the unrolled length.
pub struct Roll {
    /// The rolled solid.
    pub solid: ExactSolid,
    /// The sheet model whose flat pattern is the roll's.
    pub model: SheetModel,
    /// Neutral radius (mm).
    pub radius: Real,
    /// Unrolled length (mm).
    pub length: Real,
}

/// Distance of `q` from the line through `a` along unit `u`.
fn dist(q: Vec2, a: Vec2, u: Vec2) -> Real {
    (q - a).cross(u).abs()
}

/// The area centroid of a region (outer loop less its holes).
fn centroid(r: &Region2) -> Option<Vec2> {
    let mut area = 0.0;
    let mut c = Vec2::new(0.0, 0.0);
    for (lp, sign) in std::iter::once((&r.outer, 1.0)).chain(r.holes.iter().map(|h| (h, 1.0))) {
        let p = lp.polyline(1e-4);
        let n = p.len();
        for k in 0..n {
            let (a, b) = (p[k], p[(k + 1) % n]);
            let w = a.cross(b) * sign;
            area += w;
            c += (a + b) * w;
        }
    }
    (area.abs() > 1e-12).then(|| c * (1.0 / (3.0 * area)))
}

/// Rolls the open chain `pts` (sketch coordinates of `plane`) about the axis
/// `axis` (two sketch points) by `angle` from `start` (radians, the
/// kernel's sense); `flip` / `centered` place the thickness as Contour
/// Flange does.
#[allow(clippy::too_many_arguments)]
pub fn contour_roll(
    prefix: &str,
    rv: &RuleValues,
    plane: &Plane,
    pts: &[[Real; 2]],
    flip: bool,
    centered: bool,
    axis: ([Real; 2], [Real; 2]),
    start: Real,
    angle: Real,
    unroll: RollUnroll,
) -> Result<Roll, String> {
    if !(angle.is_finite() && angle > 1e-9) || angle > std::f64::consts::TAU + 1e-9 {
        return Err("the rolled angle must be more than 0° and at most 360°".into());
    }
    if pts.len() > 2 && angle > std::f64::consts::TAU - 1e-9 {
        return Err("360° rolls only a profile of one straight line".into());
    }
    let a = Vec2::new(axis.0[0], axis.0[1]);
    let u = (Vec2::new(axis.1[0], axis.1[1]) - a)
        .normalized()
        .ok_or("the axis has no length")?;
    // The section: the face of a thin Contour Flange lying in the sketch plane.
    let (thin, _) = contour(prefix, rv, plane, pts, (0.0, 1.0), flip, centered)?;
    let b = super::super::to_brep(std::slice::from_ref(&thin))?;
    let pl = super::plane_of(plane)?;
    let n = pl.normal();
    // Every face of it in the sketch plane (one per panel and bend), joined.
    let faces: Vec<_> = b
        .faces()
        .map(|(f, _)| f)
        .filter(|&f| {
            face_plane(&b, f).is_some_and(|(m, o)| {
                m.cross(n).len() < 1e-9 && (o - pl.origin).dot(n).abs() < 1e-6
            })
        })
        .collect();
    let mut parts = Vec::with_capacity(faces.len());
    for f in faces {
        parts.push(region_of(&b, f, &pl)?);
    }
    // One at a time: the second set's regions are not joined with each other.
    let mut joined: Vec<Region2> = Vec::new();
    for r in parts {
        joined = if joined.is_empty() {
            vec![r]
        } else {
            solidcraft_kernel::planar_regions_boolean(
                &joined,
                std::slice::from_ref(&r),
                solidcraft_kernel::BoolOp::Union,
                1e-7,
            )
            .map_err(|e| e.to_string())?
        };
    }
    if joined.len() != 1 {
        return Err(format!(
            "the profile's section came in {} pieces",
            joined.len()
        ));
    }
    let region = joined.remove(0);
    let poly = region.outer.polyline(1e-4);
    let side = |q: Vec2| (q - a).cross(u);
    let (lo, hi) = poly.iter().fold((f64::MAX, f64::MIN), |(l, h), q| {
        (l.min(side(*q)), h.max(side(*q)))
    });
    if lo < -1e-6 && hi > 1e-6 {
        return Err("the profile crosses the axis".into());
    }
    if lo.abs().min(hi.abs()) < 1e-6 && lo.abs().max(hi.abs()) < 1e-6 {
        return Err("the profile lies on the axis".into());
    }
    let t = rv.thickness;
    let radius = match unroll {
        RollUnroll::Centroid => dist(centroid(&region).ok_or("the section has no area")?, a, u),
        RollUnroll::Custom(p, q) => {
            let (p, q) = (Vec2::new(p[0], p[1]), Vec2::new(q[0], q[1]));
            if (q - p).cross(u).abs() > 1e-6 * (q - p).len().max(1.0) {
                return Err("the neutral line must be parallel to the axis".into());
            }
            dist(p, a, u)
        }
        RollUnroll::Radius(r) => r,
        RollUnroll::KFactor(k) => {
            if pts.len() > 2 {
                return Err("K-Factor unrolls a profile of one straight line".into());
            }
            let inner = poly.iter().map(|q| dist(*q, a, u)).fold(f64::MAX, f64::min);
            inner + k * t
        }
        RollUnroll::Length(_) => 0.0,
    };
    let length = match unroll {
        RollUnroll::Length(l) => l,
        _ => angle * radius,
    };
    if !(length.is_finite() && length > 1e-6) {
        return Err("the unrolled length must be positive".into());
    }
    let (_, model) = contour(prefix, rv, plane, pts, (0.0, length), flip, centered)?;
    let prof = super::super::ExactProfile::from_region(region);
    let solid = super::super::revolve(
        plane,
        &prof,
        nalgebra::Point2::new(a.x, a.y),
        nalgebra::Vector2::new(u.x, u.y),
        start,
        angle,
        prefix,
    )?;
    Ok(Roll {
        solid,
        model,
        radius,
        length,
    })
}
