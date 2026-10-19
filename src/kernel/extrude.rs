//! Extrusion of a planar profile into a solid (the kernel side of the
//! Extrude feature).
//!
//! The profile is given in the `(u, v)` coordinates of a [`Plane`]: an outer
//! loop (counter-clockwise about the plane normal) and holes (clockwise),
//! each a closed chain of [`ProfileSegment`]s. The solid is swept through a
//! list of [`ExtrudeLevel`]s along the plane normal: each level has an
//! offset and an *inset* — how far the profile is pushed outward there
//! (taper). Two levels make a plain or tapered extrusion; three make a
//! two-sided one whose taper kinks at the middle level.
//!
//! The solid gets
//!
//! - a **Start** face at the first level and an **End** face at the last;
//! - per segment and band between levels a **Side** face: a plane for
//!   lines, a [`Cylinder`] (or [`Cone`] when tapered) for arcs, an
//!   [`ExtrusionSurface`] for splines (no taper);
//! - persistent names built from the feature id and each segment's name,
//!   e.g. `Extrusion1:Side:3v1:0` (band 2: `Side2:…`), which do not depend
//!   on the sizes.

use std::f64::consts::{PI, TAU};

use super::{
    BRep, BSplineCurve, Circle, Coedge, Cone, Curve, Cylinder, EdgeId, ExtrusionSurface, Interval,
    KernelError, KernelResult, Loop, Plane, Point3, Real, Sense, Shell, SolidId, Surface,
    UnitVector3, Vector3, VertexId,
};
use nalgebra as na;

type P2 = na::Point2<Real>;
type V2 = na::Vector2<Real>;

/// Geometry of a profile segment in plane coordinates.
#[derive(Clone, Debug, PartialEq)]
pub enum Segment2 {
    /// Straight segment `a → b`.
    Line {
        /// Start.
        a: P2,
        /// End.
        b: P2,
    },
    /// Arc around `c` from angle `a0` over the signed `sweep` (positive =
    /// counter-clockwise about the plane normal).
    Arc {
        /// Centre.
        c: P2,
        /// Radius.
        r: Real,
        /// Start angle.
        a0: Real,
        /// Signed sweep.
        sweep: Real,
    },
    /// Clamped B-spline through its control points (uniform, degree
    /// `min(3, n − 1)`, when `knots` is empty), run backwards if `reversed`.
    Spline {
        /// Control points.
        points: Vec<P2>,
        /// Traversed from the last control point to the first.
        reversed: bool,
        /// Full knot vector (empty = clamped uniform).
        knots: Vec<Real>,
    },
}

impl Segment2 {
    fn reversed(&self) -> Segment2 {
        match self {
            Segment2::Line { a, b } => Segment2::Line { a: *b, b: *a },
            Segment2::Arc { c, r, a0, sweep } => Segment2::Arc {
                c: *c,
                r: *r,
                a0: a0 + sweep,
                sweep: -sweep,
            },
            Segment2::Spline {
                points,
                reversed,
                knots,
            } => Segment2::Spline {
                points: points.clone(),
                reversed: !reversed,
                knots: knots.clone(),
            },
        }
    }

    pub(super) fn start(&self) -> P2 {
        match self {
            Segment2::Line { a, .. } => *a,
            Segment2::Arc { c, r, a0, .. } => c + V2::new(a0.cos(), a0.sin()) * *r,
            Segment2::Spline {
                points, reversed, ..
            } => {
                if *reversed {
                    points[points.len() - 1]
                } else {
                    points[0]
                }
            }
        }
    }

    fn end(&self) -> P2 {
        self.reversed().start()
    }

    /// Unit tangent (in traversal direction) at the start or the end.
    fn tangent(&self, at_end: bool) -> V2 {
        match self {
            Segment2::Line { a, b } => (b - a).normalize(),
            Segment2::Arc { a0, sweep, .. } => {
                let t = if at_end { a0 + sweep } else { *a0 };
                V2::new(-t.sin(), t.cos()) * sweep.signum()
            }
            Segment2::Spline { .. } => {
                let (p, q) = if at_end {
                    (self.point(0.999), self.end())
                } else {
                    (self.start(), self.point(0.001))
                };
                (q - p).normalize()
            }
        }
    }

    pub(super) fn point(&self, s: Real) -> P2 {
        match self {
            Segment2::Line { a, b } => a + (b - a) * s,
            Segment2::Arc { c, r, a0, sweep } => {
                let t = a0 + sweep * s;
                c + V2::new(t.cos(), t.sin()) * *r
            }
            Segment2::Spline {
                points,
                reversed,
                knots,
            } => {
                let s = if *reversed { 1.0 - s } else { s };
                let cvs = points.iter().map(|p| Point3::new(p.x, p.y, 0.0)).collect();
                match BSplineCurve::sketch_spline(cvs, knots) {
                    Ok(c) => {
                        let d = c.domain();
                        let q = c.point_at(d.min + (d.max - d.min) * s);
                        P2::new(q.x, q.y)
                    }
                    Err(_) => points[0],
                }
            }
        }
    }
}

/// One segment of a profile loop with the name its topology is called by.
#[derive(Clone, Debug, PartialEq)]
pub struct ProfileSegment {
    /// Geometry, in loop direction.
    pub geometry: Segment2,
    /// Stable name (e.g. the sketch curve it comes from), unique in the profile.
    pub name: String,
}

/// A cross-section level of an extrusion.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExtrudeLevel {
    /// Offset along the plane normal (mm).
    pub offset: Real,
    /// Outward offset of the profile at this level (mm): positive grows the
    /// section (outer loop out, holes in), as a positive taper does.
    pub inset: Real,
}

/// Right-hand normal of a direction (outward for a CCW loop).
fn right(t: V2) -> V2 {
    V2::new(t.y, -t.x)
}

fn wrap(a: Real) -> Real {
    let w = (a + PI).rem_euclid(TAU) - PI;
    if w <= -PI { w + TAU } else { w }
}

/// Intersections of two segment carriers (infinite line / full circle).
fn carrier_hits(a: &Segment2, b: &Segment2) -> Vec<P2> {
    let circle = |s: &Segment2| match s {
        Segment2::Arc { c, r, .. } => Some((*c, *r)),
        _ => None,
    };
    let line = |s: &Segment2| match s {
        Segment2::Line { a, b } => Some((*a, *b)),
        _ => None,
    };
    match (line(a), line(b), circle(a), circle(b)) {
        (Some((p, q)), Some((r, s)), ..) => {
            let (d, e) = (q - p, s - r);
            let den = d.perp(&e);
            if den.abs() < 1e-14 {
                return Vec::new();
            }
            vec![p + d * ((r - p).perp(&e) / den)]
        }
        (Some((p, q)), None, _, Some((c, r))) | (None, Some((p, q)), Some((c, r)), _) => {
            let d = (q - p).normalize();
            let f = p - c;
            let bq = f.dot(&d);
            let disc = bq * bq - (f.norm_squared() - r * r);
            if disc < -1e-12 {
                return Vec::new();
            }
            let s = disc.max(0.0).sqrt();
            vec![p + d * (-bq - s), p + d * (-bq + s)]
        }
        (None, None, Some((c1, r1)), Some((c2, r2))) => {
            let d = (c2 - c1).norm();
            if d < 1e-14 {
                return Vec::new();
            }
            let a = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
            let h = (r1 * r1 - a * a).max(0.0).sqrt();
            let u = (c2 - c1) / d;
            let m = c1 + u * a;
            let n = V2::new(-u.y, u.x);
            vec![m + n * h, m - n * h]
        }
        _ => Vec::new(),
    }
}

/// Offsets a closed loop to the right of its direction by `delta` (outward
/// for counter-clockwise outer loops, into the hole for clockwise holes).
/// Corners are re-intersected; smooth joins stay smooth.
fn offset_loop(lp: &[ProfileSegment], delta: Real) -> KernelResult<Vec<ProfileSegment>> {
    if delta == 0.0 {
        return Ok(lp.to_vec());
    }
    let too_much = KernelError::Degenerate("taper too large: the profile collapses");
    // Offset carriers.
    let mut moved: Vec<Segment2> = Vec::with_capacity(lp.len());
    for s in lp {
        moved.push(match &s.geometry {
            Segment2::Line { a, b } => {
                let n = right((b - a).normalize()) * delta;
                Segment2::Line { a: a + n, b: b + n }
            }
            Segment2::Arc { c, r, a0, sweep } => {
                let r2 = r + delta * sweep.signum();
                if r2 <= 1e-9 {
                    return Err(too_much);
                }
                Segment2::Arc {
                    c: *c,
                    r: r2,
                    a0: *a0,
                    sweep: *sweep,
                }
            }
            Segment2::Spline { .. } => {
                return Err(KernelError::Unsupported("taper of spline profiles"));
            }
        });
    }
    // New corner points: where each segment meets the next.
    let m = lp.len();
    let mut corner = Vec::with_capacity(m);
    for i in 0..m {
        let j = (i + 1) % m;
        let (gi, gj) = (&lp[i].geometry, &lp[j].geometry);
        let v = gj.start();
        let (ti, tj) = (gi.tangent(true), gj.tangent(false));
        let p = if ti.perp(&tj).abs() < 1e-9 && ti.dot(&tj) > 0.0 {
            v + right(ti) * delta // smooth join
        } else {
            carrier_hits(&moved[i], &moved[j])
                .into_iter()
                .min_by(|a, b| (a - v).norm().total_cmp(&(b - v).norm()))
                .ok_or(KernelError::Degenerate("taper: offset corner not found"))?
        };
        corner.push(p);
    }
    // Rebuild each segment between its new corners.
    let mut out = Vec::with_capacity(m);
    for i in 0..m {
        let (start, end) = (corner[(i + m - 1) % m], corner[i]);
        let geometry = match (&lp[i].geometry, &moved[i]) {
            (Segment2::Line { a, b }, _) => {
                if (end - start).dot(&(b - a)) <= 1e-9 {
                    return Err(too_much);
                }
                Segment2::Line { a: start, b: end }
            }
            (_, Segment2::Arc { c, r, sweep, .. }) => {
                let a0 = (start - c).y.atan2((start - c).x);
                let full = sweep.abs() > TAU - 1e-9;
                let sweep2 = if full {
                    *sweep
                } else {
                    let a1 = (end - c).y.atan2((end - c).x);
                    sweep + wrap(a1 - a0 - sweep)
                };
                if sweep2 * sweep <= 1e-12 {
                    return Err(too_much);
                }
                Segment2::Arc {
                    c: *c,
                    r: *r,
                    a0,
                    sweep: sweep2,
                }
            }
            // Splines were rejected above.
            _ => return Err(KernelError::Unsupported("taper of spline profiles")),
        };
        out.push(ProfileSegment {
            geometry,
            name: lp[i].name.clone(),
        });
    }
    Ok(out)
}

/// The extrusion frame: plane, unit direction (along increasing level).
struct Frame<'a> {
    plane: &'a Plane,
    dir: UnitVector3,
}

impl Frame<'_> {
    fn to3(&self, p: P2, offset: Real) -> Point3 {
        self.plane.point_at(p.x, p.y) + offset * self.plane.normal.into_inner()
    }
}

impl BRep {
    /// Extrudes the profile `outer` (with `holes`) through `levels` (at
    /// least two, offsets strictly monotonic) along `plane`'s normal, as a
    /// new solid named `prefix:…` (see the module docs).
    pub fn extrude_profile(
        &mut self,
        plane: &Plane,
        outer: &[ProfileSegment],
        holes: &[Vec<ProfileSegment>],
        levels: &[ExtrudeLevel],
        prefix: &str,
    ) -> KernelResult<SolidId> {
        if levels.len() < 2 {
            return Err(KernelError::Degenerate("an extrusion needs two levels"));
        }
        let up = levels[1].offset > levels[0].offset;
        if levels.windows(2).any(|w| {
            let h = w[1].offset - w[0].offset;
            !h.is_finite() || h.abs() <= self.tolerance.linear || (h > 0.0) != up
        }) {
            return Err(KernelError::Degenerate("extrusion distance is zero"));
        }
        if outer.is_empty() || holes.iter().any(Vec::is_empty) {
            return Err(KernelError::Degenerate("empty profile loop"));
        }
        // Per level, the loops offset by its inset, then oriented so that
        // they run counter-clockwise about the extrusion direction.
        let base: Vec<&[ProfileSegment]> = std::iter::once(outer)
            .chain(holes.iter().map(Vec::as_slice))
            .collect();
        let mut sections: Vec<Vec<Vec<ProfileSegment>>> = Vec::new();
        for lv in levels {
            let mut loops = Vec::new();
            for lp in &base {
                let mut l = offset_loop(lp, lv.inset)?;
                if !up {
                    l = l
                        .iter()
                        .rev()
                        .map(|s| ProfileSegment {
                            geometry: s.geometry.reversed(),
                            name: s.name.clone(),
                        })
                        .collect();
                }
                loops.push(l);
            }
            sections.push(loops);
        }
        let n = plane.normal.into_inner();
        let frame = Frame {
            plane,
            dir: UnitVector3::new_normalize(if up { n } else { -n }),
        };
        let last = levels.len() - 1;
        let level_tag = |k: usize| match k {
            0 => "S".to_owned(),
            k if k == last => "E".to_owned(),
            k => format!("M{k}"),
        };
        let band_tag = |b: usize| {
            if b == 0 {
                String::new()
            } else {
                format!("{}", b + 1)
            }
        };

        let mut faces = Vec::new();
        let mut caps: [Vec<Loop>; 2] = [Vec::new(), Vec::new()];
        for li in 0..base.len() {
            let m = base[li].len();
            // Vertices and ring edges at every level.
            let mut verts: Vec<Vec<VertexId>> = Vec::new();
            let mut rings: Vec<Vec<(EdgeId, Sense)>> = Vec::new();
            for (k, lv) in levels.iter().enumerate() {
                let lp = &sections[k][li];
                let vs: Vec<VertexId> = lp
                    .iter()
                    .map(|s| {
                        let v = self.add_vertex(frame.to3(s.geometry.start(), lv.offset));
                        self.vertices[v].name =
                            Some(format!("{prefix}:V{}:{}", level_tag(k), s.name));
                        v
                    })
                    .collect();
                let mut ring = Vec::with_capacity(m);
                for (i, s) in lp.iter().enumerate() {
                    let e =
                        self.segment_edge(&frame, &s.geometry, lv.offset, vs[i], vs[(i + 1) % m])?;
                    let tag = match k {
                        0 => "StartEdge".to_owned(),
                        k if k == last => "EndEdge".to_owned(),
                        k => format!("MidEdge{k}"),
                    };
                    self.edges[e.0].name = Some(format!("{prefix}:{tag}:{}", s.name));
                    ring.push(e);
                }
                verts.push(vs);
                rings.push(ring);
            }
            // Side faces, band by band.
            for b in 0..last {
                let seams: Vec<EdgeId> = (0..m)
                    .map(|i| {
                        let e = self.add_line_edge(verts[b][i], verts[b + 1][i])?;
                        self.edges[e].name = Some(format!(
                            "{prefix}:Seam{}:{}",
                            band_tag(b),
                            sections[b][li][i].name
                        ));
                        Ok(e)
                    })
                    .collect::<KernelResult<_>>()?;
                for i in 0..m {
                    let (lo, hi) = (rings[b][i], rings[b + 1][i]);
                    let side = Loop {
                        coedges: vec![
                            Coedge {
                                edge: lo.0,
                                sense: lo.1,
                            },
                            Coedge {
                                edge: seams[(i + 1) % m],
                                sense: Sense::Same,
                            },
                            Coedge {
                                edge: hi.0,
                                sense: hi.1.flipped(),
                            },
                            Coedge {
                                edge: seams[i],
                                sense: Sense::Reversed,
                            },
                        ],
                    };
                    let (g0, g1) = (
                        &sections[b][li][i].geometry,
                        &sections[b + 1][li][i].geometry,
                    );
                    let (surface, sense) =
                        self.side_surface(&frame, g0, g1, levels[b].offset, levels[b + 1].offset)?;
                    let s = self.add_surface(surface);
                    let f = self.add_face(s, side, Vec::new(), sense)?;
                    self.faces[f].name = Some(format!(
                        "{prefix}:Side{}:{}",
                        band_tag(b),
                        sections[b][li][i].name
                    ));
                    faces.push(f);
                }
            }
            // End cap: loop direction; Start cap: reversed (seen from outside).
            caps[1].push(Loop {
                coedges: rings[last]
                    .iter()
                    .map(|&(edge, sense)| Coedge { edge, sense })
                    .collect(),
            });
            caps[0].push(Loop {
                coedges: rings[0]
                    .iter()
                    .rev()
                    .map(|&(edge, sense)| Coedge {
                        edge,
                        sense: sense.flipped(),
                    })
                    .collect(),
            });
        }
        for (k, name, loops) in [(0, "Start", &caps[0]), (last, "End", &caps[1])] {
            let outward = if k == 0 {
                -frame.dir.into_inner()
            } else {
                frame.dir.into_inner()
            };
            let origin = frame.to3(P2::origin(), levels[k].offset);
            let p = Plane::new(origin, outward, plane.x_axis.into_inner())?;
            let s = self.add_surface(Surface::Plane(p));
            let f = self.add_face(s, loops[0].clone(), loops[1..].to_vec(), Sense::Same)?;
            self.faces[f].name = Some(format!("{prefix}:{name}"));
            faces.push(f);
        }
        self.add_solid(vec![Shell { faces }])
    }

    /// The edge of a segment at `offset`, running `from → to` in loop
    /// direction (returned with the sense that achieves that).
    fn segment_edge(
        &mut self,
        frame: &Frame,
        seg: &Segment2,
        offset: Real,
        from: VertexId,
        to: VertexId,
    ) -> KernelResult<(EdgeId, Sense)> {
        match seg {
            Segment2::Line { .. } => Ok((self.add_line_edge(from, to)?, Sense::Same)),
            Segment2::Arc { c, r, sweep, .. } => {
                let c3 = frame.to3(*c, offset);
                let s3 = frame.to3(seg.start(), offset);
                let axis = frame.plane.normal.into_inner() * sweep.signum();
                let circle = Circle::new(c3, axis, s3 - c3, *r)?;
                let curve = self.add_curve(Curve::Circle(circle));
                let range = Interval::new(0.0, sweep.abs())?;
                Ok((self.add_edge(curve, from, to, range)?, Sense::Same))
            }
            Segment2::Spline {
                points,
                reversed,
                knots,
            } => {
                let curve = spline_at(frame, points, knots, offset)?;
                let range = curve.domain();
                let id = self.add_curve(Curve::BSpline(curve));
                if *reversed {
                    Ok((self.add_edge(id, to, from, range)?, Sense::Reversed))
                } else {
                    Ok((self.add_edge(id, from, to, range)?, Sense::Same))
                }
            }
        }
    }

    /// Surface of the side face between segment `lo` (at offset `h0`) and
    /// its copy `hi` (at `h1`), and the face's sense.
    fn side_surface(
        &self,
        frame: &Frame,
        lo: &Segment2,
        hi: &Segment2,
        h0: Real,
        h1: Real,
    ) -> KernelResult<(Surface, Sense)> {
        Ok(match (lo, hi) {
            (Segment2::Line { a, b }, Segment2::Line { a: a2, .. }) => {
                let (a3, b3, c3) = (frame.to3(*a, h0), frame.to3(*b, h0), frame.to3(*a2, h1));
                let t: Vector3 = b3 - a3;
                (
                    Surface::Plane(Plane::new(a3, t.cross(&(c3 - a3)), t)?),
                    Sense::Same,
                )
            }
            (Segment2::Arc { c, r, sweep, .. }, Segment2::Arc { r: r2, .. }) => {
                let c3 = frame.to3(*c, h0);
                let x = frame.to3(lo.start(), h0) - c3;
                let height = (h1 - h0).abs();
                let surface = if (r2 - r).abs() < 1e-12 {
                    Surface::Cylinder(Cylinder::new(c3, frame.dir.into_inner(), x, *r)?)
                } else {
                    let half = ((r2 - r) / height).atan();
                    Surface::Cone(Cone::new(c3, frame.dir.into_inner(), x, *r, half)?)
                };
                // Counter-clockwise about the direction: the material is
                // inside the arc, so the outward normal points away from it.
                let ccw = *sweep * frame.plane.normal.dot(&frame.dir) > 0.0;
                (surface, if ccw { Sense::Same } else { Sense::Reversed })
            }
            (
                Segment2::Spline {
                    points,
                    reversed,
                    knots,
                },
                _,
            ) => {
                let curve = spline_at(frame, points, knots, h0)?;
                let surface = ExtrusionSurface {
                    curve,
                    direction: frame.dir,
                };
                let sense = if *reversed {
                    Sense::Reversed
                } else {
                    Sense::Same
                };
                (Surface::Extrusion(surface), sense)
            }
            _ => {
                return Err(KernelError::InvalidTopology(
                    "mismatched side segments".into(),
                ));
            }
        })
    }
}

fn spline_at(
    frame: &Frame,
    points: &[P2],
    knots: &[Real],
    offset: Real,
) -> KernelResult<BSplineCurve> {
    let cvs = points.iter().map(|p| frame.to3(*p, offset)).collect();
    BSplineCurve::sketch_spline(cvs, knots)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::TessellationOptions;
    use std::f64::consts::{PI, TAU};

    fn p(x: Real, y: Real) -> na::Point2<Real> {
        na::Point2::new(x, y)
    }

    /// Two plain levels: from `a` to `b`.
    fn lv(a: Real, b: Real) -> Vec<ExtrudeLevel> {
        vec![
            ExtrudeLevel {
                offset: a,
                inset: 0.0,
            },
            ExtrudeLevel {
                offset: b,
                inset: 0.0,
            },
        ]
    }

    fn seg(geometry: Segment2, name: &str) -> ProfileSegment {
        ProfileSegment {
            geometry,
            name: name.into(),
        }
    }

    fn rect(w: Real, h: Real) -> Vec<ProfileSegment> {
        let c = [p(0.0, 0.0), p(w, 0.0), p(w, h), p(0.0, h)];
        (0..4)
            .map(|i| {
                seg(
                    Segment2::Line {
                        a: c[i],
                        b: c[(i + 1) % 4],
                    },
                    &format!("L{i}"),
                )
            })
            .collect()
    }

    fn circle(c: na::Point2<Real>, r: Real, ccw: bool) -> Vec<ProfileSegment> {
        let sweep = if ccw { TAU } else { -TAU };
        vec![seg(
            Segment2::Arc {
                c,
                r,
                a0: 0.0,
                sweep,
            },
            "C",
        )]
    }

    /// Volume of a closed triangle mesh (divergence theorem); positive iff
    /// every face is wound outward.
    fn volume(b: &BRep, s: SolidId) -> Real {
        let m = b
            .tessellate_solid(s, &TessellationOptions::default())
            .unwrap()
            .faces;
        m.triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| m.positions[i as usize].coords);
                a.dot(&b.cross(&c)) / 6.0
            })
            .sum()
    }

    #[test]
    fn plate_with_a_hole_is_valid_and_has_the_right_volume() {
        for distance in [10.0, -10.0] {
            let mut b = BRep::new();
            let s = b
                .extrude_profile(
                    &Plane::xy(),
                    &rect(40.0, 20.0),
                    &[circle(p(10.0, 10.0), 4.0, false)],
                    &lv(0.0, distance),
                    "Extrusion1",
                )
                .unwrap();
            let st = b.validate_solid(s).unwrap();
            assert_eq!((st.faces, st.rings), (4 + 1 + 2, 2));
            assert_eq!(st.genus(), Some(1), "a plate with one hole");
            let v = volume(&b, s);
            let exact = (800.0 - PI * 16.0) * 10.0;
            assert!(
                (v - exact).abs() < 0.01 * exact,
                "{distance}: {v} vs {exact}"
            );
            assert!(b.find_face("Extrusion1:Side:C").is_some());
            assert!(b.find_face("Extrusion1:End").is_some());
        }
    }

    #[test]
    fn tapered_plate_with_hole_and_two_sided_taper() {
        let (w, h, r, height) = (40.0, 20.0, 4.0, 10.0);
        let delta: Real = height * 5f64.to_radians().tan();
        let mut b = BRep::new();
        let levels = [
            ExtrudeLevel {
                offset: 0.0,
                inset: 0.0,
            },
            ExtrudeLevel {
                offset: height,
                inset: delta,
            },
        ];
        let s = b
            .extrude_profile(
                &Plane::xy(),
                &rect(w, h),
                &[circle(p(10.0, 10.0), r, false)],
                &levels,
                "T",
            )
            .unwrap();
        let st = b.validate_solid(s).unwrap();
        assert_eq!(st.genus(), Some(1));
        // Frustum of the rectangle minus a cone (the hole shrinks).
        let outer = height * (w * h + (w + h) * delta + 4.0 / 3.0 * delta * delta);
        let hole = height * PI * (r * r - r * delta + delta * delta / 3.0);
        let v = volume(&b, s);
        assert!(
            (v - (outer - hole)).abs() < 0.005 * v,
            "{v} vs {}",
            outer - hole
        );
        assert!(matches!(
            b.surface(b.face(b.find_face("T:Side:C").unwrap()).unwrap().surface),
            Some(Surface::Cone(_))
        ));

        // Symmetric with taper: three levels, kinked at the sketch plane.
        let mut b = BRep::new();
        let levels = [
            ExtrudeLevel {
                offset: -5.0,
                inset: 0.5,
            },
            ExtrudeLevel {
                offset: 0.0,
                inset: 0.0,
            },
            ExtrudeLevel {
                offset: 5.0,
                inset: 0.5,
            },
        ];
        let s = b
            .extrude_profile(&Plane::xy(), &rect(10.0, 10.0), &[], &levels, "S")
            .unwrap();
        b.validate_solid(s).unwrap();
        let v = volume(&b, s);
        let half = 5.0 * (100.0 + 20.0 * 0.5 + 4.0 / 3.0 * 0.25);
        assert!((v - 2.0 * half).abs() < 1e-6, "{v}");
        assert!(b.find_face("S:Side2:L0").is_some());

        // A taper that collapses the profile is refused.
        let collapse = [
            ExtrudeLevel {
                offset: 0.0,
                inset: 0.0,
            },
            ExtrudeLevel {
                offset: 10.0,
                inset: -6.0,
            },
        ];
        assert!(
            BRep::new()
                .extrude_profile(&Plane::xy(), &rect(10.0, 10.0), &[], &collapse, "X")
                .is_err()
        );
    }

    #[test]
    fn slot_and_spline_profiles() {
        // A slot: two lines and two half circles.
        let slot = vec![
            seg(
                Segment2::Line {
                    a: p(0.0, -2.0),
                    b: p(10.0, -2.0),
                },
                "B",
            ),
            seg(
                Segment2::Arc {
                    c: p(10.0, 0.0),
                    r: 2.0,
                    a0: -PI / 2.0,
                    sweep: PI,
                },
                "R",
            ),
            seg(
                Segment2::Line {
                    a: p(10.0, 2.0),
                    b: p(0.0, 2.0),
                },
                "T",
            ),
            seg(
                Segment2::Arc {
                    c: p(0.0, 0.0),
                    r: 2.0,
                    a0: PI / 2.0,
                    sweep: PI,
                },
                "L",
            ),
        ];
        let mut b = BRep::new();
        let s = b
            .extrude_profile(&Plane::xy(), &slot, &[], &lv(0.0, 5.0), "E")
            .unwrap();
        b.validate_solid(s).unwrap();
        let v = volume(&b, s);
        let exact = (40.0 + PI * 4.0) * 5.0;
        assert!((v - exact).abs() < 0.01 * exact, "{v} vs {exact}");

        // A line closed by a spline (reversed in the loop).
        let pts = vec![p(10.0, 0.0), p(10.0, 8.0), p(0.0, 8.0), p(0.0, 0.0)];
        let lp = vec![
            seg(
                Segment2::Line {
                    a: p(0.0, 0.0),
                    b: p(10.0, 0.0),
                },
                "A",
            ),
            seg(
                Segment2::Spline {
                    points: pts,
                    reversed: false,
                    knots: Vec::new(),
                },
                "S",
            ),
        ];
        let mut b = BRep::new();
        let s = b
            .extrude_profile(&Plane::xy(), &lp, &[], &lv(0.0, 3.0), "E")
            .unwrap();
        b.validate_solid(s).unwrap();
        assert!(volume(&b, s) > 0.0, "outward faces");
        assert!(
            b.extrude_profile(&Plane::xy(), &lp, &[], &lv(0.0, 0.0), "X")
                .is_err()
        );
    }
}
