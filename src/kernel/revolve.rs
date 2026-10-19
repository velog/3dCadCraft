//! Revolve: a planar profile swept around an axis in its plane
//! (`docs/spec/2.2-revolve.md`).
//!
//! The sweep is built as tagged polygons (one tag per profile segment, plus
//! the two caps of a partial revolve) and rebuilt by
//! [`BRep::add_polygons`]: segments perpendicular to the axis become planar
//! faces, the others facets of their exact surface — cylinder (parallel to
//! the axis), cone (oblique), sphere (arc centred on the axis) or torus.
//!
//! Names: `<prefix>:Side:<segment name>`, `<prefix>:Start`, `<prefix>:End`.

use std::f64::consts::TAU;

use nalgebra as na;

use super::csg::{self, CsgPolygon};
use super::extrude::{ProfileSegment, Segment2};
use super::facet::FaceTag;
use super::{
    BRep, Cone, Cylinder, KernelError, KernelResult, Plane, Point3, Real, SolidId, Sphere, Surface,
    Torus, UnitVector3, Vector3, triangulate_with_holes,
};

type P2 = na::Point2<Real>;
type V2 = na::Vector2<Real>;

/// The axis of a revolve, in profile-plane coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RevolveAxis {
    /// A point on the axis.
    pub origin: P2,
    /// Its direction (need not be unit).
    pub dir: V2,
}

impl BRep {
    /// Revolves the profile `outer` (with `holes`) on `plane` around `axis`
    /// from `start` over the signed `sweep` (radians, right-handed about the
    /// axis direction; `|sweep| ≥ 2π` is a full revolution). With `surface`
    /// only the swept faces are made (an open surface body, no caps).
    #[allow(clippy::too_many_arguments)]
    pub fn revolve_profile(
        &mut self,
        plane: &Plane,
        outer: &[ProfileSegment],
        holes: &[Vec<ProfileSegment>],
        axis: RevolveAxis,
        start: Real,
        sweep: Real,
        surface: bool,
        prefix: &str,
    ) -> KernelResult<SolidId> {
        let tol = self.tolerance.linear;
        let dir = axis
            .dir
            .try_normalize(1e-12)
            .ok_or(KernelError::Degenerate("the revolve axis has no direction"))?;
        if !sweep.is_finite() || sweep.abs() < 1e-9 {
            return Err(KernelError::Degenerate("the revolve angle is zero"));
        }
        if outer.is_empty() {
            return Err(KernelError::Degenerate("empty profile loop"));
        }
        let full = sweep.abs() >= TAU - 1e-9;
        let sweep = sweep.clamp(-TAU, TAU);
        // Signed distance of a profile point from the axis.
        let radial = |p: &P2| dir.perp(&(p - axis.origin));
        let loops: Vec<&[ProfileSegment]> = std::iter::once(outer)
            .chain(holes.iter().map(Vec::as_slice))
            .collect();
        // Sampled polylines per segment, and the side of the axis.
        let mut side = 0.0;
        let mut lines: Vec<Vec<(Vec<P2>, &ProfileSegment)>> = Vec::new();
        for lp in &loops {
            let mut l = Vec::new();
            for seg in lp.iter() {
                let pts = sample(&seg.geometry, tol);
                for p in &pts {
                    let r = radial(p);
                    if r.abs() > tol {
                        if side * r < 0.0 {
                            return Err(KernelError::Degenerate(
                                "the profile crosses the revolve axis",
                            ));
                        }
                        side = r.signum();
                    }
                }
                l.push((pts, seg));
            }
            lines.push(l);
        }
        if side == 0.0 {
            return Err(KernelError::Degenerate("the profile lies on the axis"));
        }
        // Material on the left of every loop: outer counter-clockwise,
        // holes clockwise.
        for (i, l) in lines.iter_mut().enumerate() {
            if (signed_area(l) > 0.0) != (i == 0) {
                l.reverse();
                for (pts, _) in l.iter_mut() {
                    pts.reverse();
                }
            }
        }
        // 3D axis and the rotation of a profile point.
        let to3 = |p: &P2| plane.point_at(p.x, p.y);
        let a0 = to3(&axis.origin);
        let ad = UnitVector3::new_normalize(to3(&(axis.origin + dir)) - a0);
        let rmax = lines
            .iter()
            .flatten()
            .flat_map(|(pts, _)| pts.iter().map(|p| radial(p).abs()))
            .fold(0.0, Real::max);
        let _ = rmax;
        // Angles on a global grid (multiples of 2π/GRID from the sketch
        // plane) plus the exact start and end: coaxial features share
        // their facet vertices, so Booleans between them stay clean.
        let angles = grid_angles(start, sweep);
        let n = angles.len() - 1;
        let rots: Vec<na::Rotation3<Real>> = angles
            .iter()
            .map(|&t| na::Rotation3::from_axis_angle(&ad, t))
            .collect();
        let at = |p: &P2, k: usize| -> Point3 { a0 + rots[k] * (to3(p) - a0) };

        let half = na::Rotation3::from_axis_angle(&ad, (angles[0] + angles[1]) / 2.0);
        let mut probe: Option<(usize, Vector3)> = None;
        let mut tags: Vec<FaceTag> = Vec::new();
        let mut polys: Vec<CsgPolygon> = Vec::new();
        for l in &lines {
            for (pts, seg) in l {
                let tag = tags.len();
                tags.push(self.revolved_tag(
                    plane,
                    &seg.geometry,
                    &axis,
                    dir,
                    ad,
                    a0,
                    prefix,
                    &seg.name,
                ));
                for w in pts.windows(2) {
                    let (p, q) = (w[0], w[1]);
                    if radial(&p).abs() <= tol && radial(&q).abs() <= tol {
                        continue; // on the axis: sweeps nothing
                    }
                    for k in 0..n {
                        let mut quad = vec![at(&p, k), at(&q, k), at(&q, k + 1), at(&p, k + 1)];
                        quad.dedup_by(|a, b| (*a - *b).norm() <= tol);
                        if quad.len() > 3 && (quad[0] - quad[quad.len() - 1]).norm() <= tol {
                            quad.pop();
                        }
                        let before = polys.len();
                        polys.extend(CsgPolygon::new(quad, tag));
                        if k == 0
                            && probe.is_none()
                            && polys.len() > before
                            && radial(&p).abs() > tol
                            && radial(&q).abs() > tol
                        {
                            // Outward in the profile plane: right of p → q.
                            let d = (q - p).normalize();
                            let out = to3(&(p + V2::new(d.y, -d.x))) - to3(&p);
                            probe = Some((before, half * out));
                        }
                    }
                }
            }
        }
        if !full && !surface {
            // Caps: the profile itself at both ends.
            let ring = |l: &Vec<(Vec<P2>, &ProfileSegment)>| -> Vec<P2> {
                let mut r: Vec<P2> = Vec::new();
                for (pts, _) in l {
                    r.extend(&pts[..pts.len() - 1]);
                }
                r
            };
            let outer2 = ring(&lines[0]);
            let holes2: Vec<Vec<P2>> = lines[1..].iter().map(ring).collect();
            let (pts, tris) = triangulate_with_holes(&outer2, &holes2);
            for (k, name) in [(0, "Start"), (n, "End")] {
                let tag = tags.len();
                tags.push(FaceTag {
                    name: Some(format!("{prefix}:{name}")),
                    surface: None,
                    curved: false,
                });
                for t in &tris {
                    polys.extend(CsgPolygon::new(
                        t.iter().map(|&i| at(&pts[i], k)).collect(),
                        tag,
                    ));
                }
            }
        }
        // Orient outward. Sides: all quads share one orientation; compare
        // one quad's normal with the profile's outward direction (to the
        // right of a segment) carried half a step around the axis.
        let caps_from = polys
            .iter()
            .position(|p| tags[p.tag].surface.is_none() && !tags[p.tag].curved)
            .unwrap_or(polys.len());
        let mut cap_polys = polys.split_off(caps_from);
        let mut side_polys = polys;
        if let Some((i, o3)) = probe
            && side_polys[i].normal.dot(&o3) < 0.0
        {
            side_polys = side_polys.into_iter().map(flipped).collect();
        }
        // Caps: the start cap faces against the sweep, the end cap with it.
        for p in &mut cap_polys {
            let is_start = tags[p.tag].name.as_deref() == Some(&format!("{prefix}:Start"));
            let c = centroid(&p.pts);
            let motion = ad.cross(&(c - a0)) * sweep.signum();
            let want = if is_start { -motion } else { motion };
            if p.normal.dot(&want) < 0.0 {
                *p = flipped(p.clone());
            }
        }
        let mut all = side_polys;
        all.extend(cap_polys);
        if !surface && csg::volume(&all) <= 0.0 {
            return Err(KernelError::Degenerate("the revolve has no volume"));
        }
        let ids = self.add_polygons(&all, &tags, surface)?;
        ids.first()
            .copied()
            .ok_or(KernelError::Degenerate("the revolve is empty"))
    }

    /// The face tag (name + exact surface) of the face a segment sweeps.
    #[allow(clippy::too_many_arguments)]
    fn revolved_tag(
        &self,
        plane: &Plane,
        g: &Segment2,
        axis: &RevolveAxis,
        dir: V2,
        ad: UnitVector3,
        a0: Point3,
        prefix: &str,
        name: &str,
    ) -> FaceTag {
        let tol = self.tolerance.linear;
        let to3 = |p: &P2| plane.point_at(p.x, p.y);
        let axial = |p: &P2| (p - axis.origin).dot(&dir);
        let radial = |p: &P2| dir.perp(&(p - axis.origin)).abs();
        // In 3D: the radial direction of the profile half-plane.
        let x3 = {
            let v = to3(&(axis.origin + V2::new(-dir.y, dir.x))) - a0;
            v - ad.into_inner() * v.dot(&ad)
        };
        let on_axis = |t: Real| a0 + ad.into_inner() * t;
        let surface = match g {
            Segment2::Line { a, b } => {
                let (ra, rb) = (radial(a), radial(b));
                let (ta, tb) = (axial(a), axial(b));
                if (ra - rb).abs() <= tol {
                    Cylinder::new(on_axis(ta), ad.into_inner(), x3, ra.max(tol))
                        .ok()
                        .map(Surface::Cylinder)
                } else if (ta - tb).abs() <= tol {
                    // ⟂ the axis: an annulus/disc, planar.
                    let n: Vector3 = ad.into_inner();
                    return FaceTag {
                        name: Some(format!("{prefix}:Side:{name}")),
                        surface: Plane::new(on_axis(ta), n, x3).ok().map(Surface::Plane),
                        curved: false,
                    };
                } else {
                    // Cone through both points; reference circle at the
                    // larger radius.
                    let (t0, r0, t1, r1) = if ra >= rb {
                        (ta, ra, tb, rb)
                    } else {
                        (tb, rb, ta, ra)
                    };
                    let half = ((r1 - r0) / (t1 - t0)).atan();
                    Cone::new(on_axis(t0), ad.into_inner(), x3, r0, half)
                        .ok()
                        .map(Surface::Cone)
                }
            }
            Segment2::Arc { c, r, .. } => {
                let rc = radial(c);
                let center = on_axis(axial(c));
                let x_axis = UnitVector3::try_new(x3, 1e-12).unwrap_or(ad);
                if rc <= tol {
                    Some(Surface::Sphere(Sphere {
                        center,
                        radius: *r,
                        axis: ad,
                        x_axis,
                    }))
                } else {
                    Some(Surface::Torus(Torus {
                        center,
                        axis: ad,
                        x_axis,
                        major: rc,
                        minor: *r,
                    }))
                }
            }
            Segment2::Spline { .. } => None,
        };
        FaceTag {
            name: Some(format!("{prefix}:Side:{name}")),
            surface,
            curved: true,
        }
    }
}

fn flipped(mut p: CsgPolygon) -> CsgPolygon {
    p.pts.reverse();
    p.normal = -p.normal;
    p.w = -p.w;
    p
}

fn centroid(pts: &[Point3]) -> Point3 {
    let s = pts.iter().fold(Vector3::zeros(), |a, p| a + p.coords);
    Point3::from(s / pts.len() as Real)
}

fn signed_area(l: &[(Vec<P2>, &ProfileSegment)]) -> Real {
    let pts: Vec<P2> = l
        .iter()
        .flat_map(|(p, _)| p[..p.len() - 1].iter().copied())
        .collect();
    (0..pts.len())
        .map(|i| pts[i].coords.perp(&pts[(i + 1) % pts.len()].coords))
        .sum::<Real>()
        / 2.0
}

/// Angular facets per full turn (3.75°).
const GRID: usize = 96;

/// `start`, the grid angles strictly inside the sweep, and the end, in
/// sweep order.
fn grid_angles(start: Real, sweep: Real) -> Vec<Real> {
    let end = start + sweep;
    let step = TAU / GRID as Real;
    let (lo, hi) = if sweep > 0.0 {
        (start, end)
    } else {
        (end, start)
    };
    let eps = step * 1e-3;
    let mut v = vec![lo];
    let mut k = (lo / step).floor() as i64 + 1;
    loop {
        let t = k as Real * step;
        if t >= hi - eps {
            break;
        }
        if t > lo + eps {
            v.push(t);
        }
        k += 1;
    }
    v.push(hi);
    if sweep < 0.0 {
        v.reverse();
    }
    v
}

/// Points along a segment (both ends included): arcs at the global angle
/// grid around their centre (plus their ends), so equal arcs of different
/// features coincide.
fn sample(g: &Segment2, _tol: Real) -> Vec<P2> {
    match g {
        Segment2::Line { a, b } => vec![*a, *b],
        Segment2::Arc { c, r, a0, sweep } => grid_angles(*a0, *sweep)
            .into_iter()
            .map(|t| c + V2::new(t.cos(), t.sin()) * *r)
            .collect(),
        Segment2::Spline { .. } => (0..=48).map(|k| g.point(k as Real / 48.0)).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    fn rect(x0: Real, y0: Real, x1: Real, y1: Real) -> Vec<ProfileSegment> {
        let c = [
            P2::new(x0, y0),
            P2::new(x1, y0),
            P2::new(x1, y1),
            P2::new(x0, y1),
        ];
        (0..4)
            .map(|i| ProfileSegment {
                geometry: Segment2::Line {
                    a: c[i],
                    b: c[(i + 1) % 4],
                },
                name: format!("L{i}"),
            })
            .collect()
    }

    fn xy() -> Plane {
        Plane::new(Point3::origin(), Vector3::z(), Vector3::x()).unwrap()
    }

    /// Volume of a solid from its tessellation.
    fn volume(b: &BRep, s: SolidId) -> Real {
        let mut tags = Vec::new();
        csg::volume(&b.solid_polygons(s, &mut tags).unwrap())
    }

    #[test]
    fn full_revolve_of_a_rectangle_is_a_tube() {
        // Rectangle x 10..20, y 0..30 around the Y axis: a tube.
        let mut b = BRep::new();
        let axis = RevolveAxis {
            origin: P2::origin(),
            dir: V2::y(),
        };
        let s = b
            .revolve_profile(
                &xy(),
                &rect(10.0, 0.0, 20.0, 30.0),
                &[],
                axis,
                0.0,
                TAU,
                false,
                "Rev1",
            )
            .unwrap();
        let st = b.validate_solid(s).unwrap();
        assert_eq!(st.genus(), Some(1), "a tube has a through-hole");
        let exact = PI * (400.0 - 100.0) * 30.0;
        let v = volume(&b, s);
        assert!((v - exact).abs() / exact < 0.01, "volume {v} vs {exact}");
        // Planar ring faces are single faces with a hole; side faces facets
        // of cylinders.
        let top = b.find_face("Rev1:Side:L2").unwrap();
        assert_eq!(b.face(top).unwrap().inner.len(), 1);
        let (_, outer_wall) = b
            .faces()
            .find(|(_, f)| f.name.as_deref() == Some("Rev1:Side:L1"))
            .unwrap();
        let ex = outer_wall.facet_of.and_then(|s| b.surface(s)).unwrap();
        assert!(matches!(ex, Surface::Cylinder(c) if (c.radius - 20.0).abs() < 1e-9));
    }

    #[test]
    fn partial_revolve_touching_the_axis_has_caps() {
        // Rectangle touching the axis, 90°: a quarter cylinder.
        let mut b = BRep::new();
        let axis = RevolveAxis {
            origin: P2::origin(),
            dir: V2::y(),
        };
        let s = b
            .revolve_profile(
                &xy(),
                &rect(0.0, 0.0, 10.0, 5.0),
                &[],
                axis,
                0.0,
                PI / 2.0,
                false,
                "R",
            )
            .unwrap();
        let st = b.validate_solid(s).unwrap();
        assert_eq!(st.genus(), Some(0));
        let exact = PI * 100.0 * 5.0 / 4.0;
        let v = volume(&b, s);
        assert!(
            v > 0.0 && (v - exact).abs() / exact < 0.01,
            "volume {v} vs {exact}"
        );
        assert!(b.find_face("R:Start").is_some() && b.find_face("R:End").is_some());
        // Negative direction is the mirror image, same volume.
        let s2 = b
            .revolve_profile(
                &xy(),
                &rect(0.0, 0.0, 10.0, 5.0),
                &[],
                axis,
                0.0,
                -PI / 2.0,
                false,
                "Q",
            )
            .unwrap();
        b.validate_solid(s2).unwrap();
        assert!((volume(&b, s2) - v).abs() < 1e-6);
        // Crossing the axis is refused.
        assert!(
            b.revolve_profile(
                &xy(),
                &rect(-5.0, 0.0, 10.0, 5.0),
                &[],
                axis,
                0.0,
                TAU,
                false,
                "X"
            )
            .is_err()
        );
    }

    #[test]
    fn circle_off_axis_makes_a_torus() {
        let mut b = BRep::new();
        let circle = vec![ProfileSegment {
            geometry: Segment2::Arc {
                c: P2::new(20.0, 0.0),
                r: 5.0,
                a0: 0.0,
                sweep: TAU,
            },
            name: "C".into(),
        }];
        let axis = RevolveAxis {
            origin: P2::origin(),
            dir: V2::y(),
        };
        let s = b
            .revolve_profile(&xy(), &circle, &[], axis, 0.0, TAU, false, "T")
            .unwrap();
        let st = b.validate_solid(s).unwrap();
        assert_eq!(st.genus(), Some(1));
        let exact = 2.0 * PI * PI * 20.0 * 25.0;
        let v = volume(&b, s);
        assert!((v - exact).abs() / exact < 0.01, "volume {v} vs {exact}");
    }
}
