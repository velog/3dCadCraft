//! Profiles and regions (Inventor: the closed areas of a sketch that
//! Extrude / Revolve pick as a *profile*).
//!
//! The normal (not construction / centerline) curves of a sketch form a
//! planar arrangement:
//!
//! 1. every line, arc and circle is split where it crosses or touches
//!    another curve (splines join only at their end points for now);
//! 2. the pieces become the edges of a planar graph; dangling edges are
//!    pruned (they bound nothing);
//! 3. the faces of the graph are traced by always turning as far right as
//!    possible, so each bounded face comes out counter-clockwise and the
//!    outer boundary of each connected piece clockwise;
//! 4. a piece lying inside a face of another piece becomes a hole of the
//!    smallest such face.
//!
//! A [`Region`] is one such face: a counter-clockwise outer loop and
//! clockwise holes — the same convention as the kernel's faces. A rectangle
//! with two circles inside gives three regions: the plate with two holes and
//! the two discs.

use std::f64::consts::TAU;

use super::modify::Geom;
use super::{Entity, EntityId, Point2, Sketch, Style, Vector2};
use crate::kernel::Real;

/// Geometry of one edge of a profile loop, in sketch coordinates.
#[derive(Clone, Debug, PartialEq)]
pub enum ProfileCurve {
    /// Straight segment `a → b`.
    Line {
        /// Start.
        a: Point2,
        /// End.
        b: Point2,
    },
    /// Arc around `c` from angle `a0` over the signed `sweep` (positive =
    /// counter-clockwise), radians.
    Arc {
        /// Centre.
        c: Point2,
        /// Radius.
        r: Real,
        /// Start angle.
        a0: Real,
        /// Signed sweep.
        sweep: Real,
    },
    /// A whole sketch spline, given by its control points; `reversed` runs
    /// it from the last control point to the first.
    Spline {
        /// Control points.
        points: Vec<Point2>,
        /// Traversed backwards.
        reversed: bool,
        /// Full knot vector (empty = clamped uniform).
        knots: Vec<Real>,
    },
}

impl ProfileCurve {
    /// Point at the normalised parameter `s ∈ [0, 1]`.
    pub fn point(&self, s: Real) -> Point2 {
        match self {
            ProfileCurve::Line { a, b } => a + (b - a) * s,
            ProfileCurve::Arc { c, r, a0, sweep } => {
                let t = a0 + sweep * s;
                c + super::Vector2::new(t.cos(), t.sin()) * *r
            }
            ProfileCurve::Spline {
                points,
                reversed,
                knots,
            } => {
                let s = if *reversed { 1.0 - s } else { s };
                spline_point(points, knots, s)
            }
        }
    }

    /// Start point.
    pub fn start(&self) -> Point2 {
        self.point(0.0)
    }

    /// End point.
    pub fn end(&self) -> Point2 {
        self.point(1.0)
    }

    /// The same curve run backwards.
    pub fn reversed(&self) -> ProfileCurve {
        match self {
            ProfileCurve::Line { a, b } => ProfileCurve::Line { a: *b, b: *a },
            ProfileCurve::Arc { c, r, a0, sweep } => ProfileCurve::Arc {
                c: *c,
                r: *r,
                a0: a0 + sweep,
                sweep: -sweep,
            },
            ProfileCurve::Spline {
                points,
                reversed,
                knots,
            } => ProfileCurve::Spline {
                points: points.clone(),
                reversed: !reversed,
                knots: knots.clone(),
            },
        }
    }

    /// Polyline from start to end, with chord error at most `tol`.
    pub fn sample(&self, tol: Real) -> Vec<Point2> {
        let n = match self {
            ProfileCurve::Line { .. } => 1,
            ProfileCurve::Arc { r, sweep, .. } => {
                let step = 2.0 * (1.0 - (tol / r).min(1.0)).acos();
                ((sweep.abs() / step.max(1e-3)).ceil() as usize).clamp(1, 1024)
            }
            ProfileCurve::Spline { points, .. } => 16 * points.len(),
        };
        (0..=n).map(|i| self.point(i as Real / n as Real)).collect()
    }
}

/// Point of a clamped uniform sketch spline at `s ∈ [0, 1]`.
fn spline_point(points: &[Point2], knots: &[Real], s: Real) -> Point2 {
    use crate::kernel::{BSplineCurve, Point3};
    let cvs = points.iter().map(|p| Point3::new(p.x, p.y, 0.0)).collect();
    match BSplineCurve::sketch_spline(cvs, knots) {
        Ok(c) => {
            let d = c.domain();
            let q = c.point_at(d.min + (d.max - d.min) * s);
            Point2::new(q.x, q.y)
        }
        Err(_) => points[0] + (points[points.len() - 1] - points[0]) * s,
    }
}

/// One edge of a profile loop and the sketch curve it comes from.
#[derive(Clone, Debug, PartialEq)]
pub struct ProfileEdge {
    /// The sketch entity this edge is (a piece of).
    pub source: EntityId,
    /// Its geometry, in loop direction.
    pub curve: ProfileCurve,
}

/// A closed chain of edges.
#[derive(Clone, Debug, PartialEq)]
pub struct ProfileLoop {
    /// Edges head to tail; the last ends where the first starts.
    pub edges: Vec<ProfileEdge>,
}

impl ProfileLoop {
    /// Closed polygon (without repeating the first point), chord error `tol`.
    pub fn polygon(&self, tol: Real) -> Vec<Point2> {
        let mut out = Vec::new();
        for e in &self.edges {
            let pts = e.curve.sample(tol);
            out.extend_from_slice(&pts[..pts.len() - 1]);
        }
        out
    }

    /// Signed area: positive when counter-clockwise.
    pub fn area(&self) -> Real {
        polygon_area(&self.polygon(1e-4))
    }
}

/// A closed area of the sketch: counter-clockwise outer loop, clockwise holes.
#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    /// Outer boundary (counter-clockwise).
    pub outer: ProfileLoop,
    /// Holes (clockwise).
    pub holes: Vec<ProfileLoop>,
}

impl Region {
    /// Area (outer minus holes).
    pub fn area(&self) -> Real {
        self.outer.area() + self.holes.iter().map(ProfileLoop::area).sum::<Real>()
    }

    /// A point strictly inside the region (the centroid of its largest
    /// triangle), e.g. to remember which region was picked.
    pub fn interior_point(&self) -> Point2 {
        let tol = 1e-3;
        let holes: Vec<Vec<Point2>> = self.holes.iter().map(|h| h.polygon(tol)).collect();
        let (pts, tris) = crate::kernel::triangulate_with_holes(&self.outer.polygon(tol), &holes);
        tris.iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| pts[i]);
                (
                    (b - a).perp(&(c - a)).abs(),
                    Point2::from((a.coords + b.coords + c.coords) / 3.0),
                )
            })
            .max_by(|x, y| x.0.total_cmp(&y.0))
            .map_or_else(|| self.outer.edges[0].curve.start(), |(_, p)| p)
    }

    /// Is `p` inside the region (inside the outer loop, outside the holes)?
    pub fn contains(&self, p: Point2) -> bool {
        let tol = 1e-3;
        point_in_polygon(&self.outer.polygon(tol), p)
            && !self
                .holes
                .iter()
                .any(|h| point_in_polygon(&h.polygon(tol), p))
    }
}

fn polygon_area(pts: &[Point2]) -> Real {
    let n = pts.len();
    (0..n)
        .map(|i| pts[i].coords.perp(&pts[(i + 1) % n].coords))
        .sum::<Real>()
        / 2.0
}

fn point_in_polygon(poly: &[Point2], p: Point2) -> bool {
    let mut inside = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a.y > p.y) != (b.y > p.y) {
            let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
            if p.x < x {
                inside = !inside;
            }
        }
    }
    inside
}

/// An edge of the planar graph.
struct GraphEdge {
    from: usize,
    to: usize,
    source: EntityId,
    curve: ProfileCurve,
}

impl Sketch {
    /// The whole curve `id` as profile geometry (a line, an arc counter-
    /// clockwise, a circle as a full turn from angle 0, a spline with its
    /// knots), e.g. as a sweep path. `None` for a missing entity.
    pub fn curve_of(&self, id: EntityId) -> Option<ProfileCurve> {
        let pos = |p| self.point(p).map(|q| q.position);
        Some(match self.entity(id)? {
            Entity::Line { start, end } => ProfileCurve::Line {
                a: pos(*start)?,
                b: pos(*end)?,
            },
            Entity::Circle { center, radius } => ProfileCurve::Arc {
                c: pos(*center)?,
                r: *radius,
                a0: 0.0,
                sweep: TAU,
            },
            Entity::Arc { center, start, end } => {
                let (c, s, e) = (pos(*center)?, pos(*start)?, pos(*end)?);
                let a0 = (s.y - c.y).atan2(s.x - c.x);
                let a1 = (e.y - c.y).atan2(e.x - c.x);
                let mut sweep = (a1 - a0).rem_euclid(TAU);
                if sweep < 1e-12 {
                    sweep = TAU;
                }
                ProfileCurve::Arc {
                    c,
                    r: (s - c).norm(),
                    a0,
                    sweep,
                }
            }
            Entity::Spline { .. } => {
                let (cv, knots) = self.spline_def(id, &|p| {
                    self.point(p)
                        .map_or(Vector2::zeros(), |q| q.position.coords)
                });
                ProfileCurve::Spline {
                    points: cv.into_iter().map(Point2::from).collect(),
                    reversed: false,
                    knots,
                }
            }
        })
    }

    /// The closed regions formed by the sketch's normal curves (see the
    /// module docs). Deterministic for a given sketch.
    pub fn regions(&self) -> Vec<Region> {
        let mut pieces = self.profile_pieces();
        let scale = pieces
            .iter()
            .flat_map(|(_, c)| [c.start(), c.end()])
            .map(|p| p.coords.amax())
            .fold(1.0, Real::max);
        let tol = 1e-7 * scale;
        // Coincident pieces (a curve drawn on a projected edge, squares
        // sharing the plate's outline) are one edge: two edges between the
        // same vertices made a zero-area face and lost the regions around.
        let same = |a: &ProfileCurve, b: &ProfileCurve| {
            let close = |p: Point2, q: Point2| (p - q).norm() <= 10.0 * tol;
            let ends = (close(a.start(), b.start()) && close(a.end(), b.end()))
                || (close(a.start(), b.end()) && close(a.end(), b.start()));
            ends && close(a.point(0.5), b.point(0.5)) && close(a.point(0.25), b.point(0.25))
                || ends && close(a.point(0.5), b.point(0.5)) && close(a.point(0.25), b.point(0.75))
        };
        let mut kept: Vec<(EntityId, ProfileCurve)> = Vec::with_capacity(pieces.len());
        for (src, c) in pieces.drain(..) {
            if !kept.iter().any(|(_, k)| same(k, &c)) {
                kept.push((src, c));
            }
        }
        let pieces = kept;

        // Vertices: piece ends merged within tolerance.
        let mut verts: Vec<Point2> = Vec::new();
        let mut vertex = |p: Point2| -> usize {
            if let Some(i) = verts.iter().position(|q| (q - p).norm() <= tol) {
                return i;
            }
            verts.push(p);
            verts.len() - 1
        };
        let mut edges: Vec<GraphEdge> = Vec::new();
        for (source, curve) in pieces {
            let (from, to) = (vertex(curve.start()), vertex(curve.end()));
            // Full circles and closed splines are loops on their own.
            let closed = matches!(curve, ProfileCurve::Arc { sweep, .. } if sweep.abs() > TAU - 1e-9)
                || matches!(&curve, ProfileCurve::Spline { points, .. } if points.len() > 3);
            if from == to && !closed {
                continue; // degenerate piece
            }
            edges.push(GraphEdge {
                from,
                to,
                source,
                curve,
            });
        }
        let nv = verts.len();

        // Prune dangling edges: they bound no area.
        let mut alive = vec![true; edges.len()];
        loop {
            let mut degree = vec![0usize; nv];
            for (e, _) in edges.iter().zip(&alive).filter(|(_, a)| **a) {
                degree[e.from] += 1;
                degree[e.to] += 1;
            }
            let mut changed = false;
            for (i, e) in edges.iter().enumerate() {
                if alive[i] && (degree[e.from] == 1 || degree[e.to] == 1) {
                    alive[i] = false;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }

        // Half-edges 2i (forward) and 2i+1 (backward), sorted around each
        // vertex by the direction they leave it in (a short step along the
        // curve, so tangent edges are told apart by their curvature).
        let half = |h: usize| -> ProfileCurve {
            let c = &edges[h / 2].curve;
            if h.is_multiple_of(2) {
                c.clone()
            } else {
                c.reversed()
            }
        };
        let origin = |h: usize| {
            let e = &edges[h / 2];
            if h.is_multiple_of(2) { e.from } else { e.to }
        };
        let mut out: Vec<Vec<(Real, usize)>> = vec![Vec::new(); nv];
        for (i, _) in edges.iter().enumerate().filter(|(i, _)| alive[*i]) {
            for h in [2 * i, 2 * i + 1] {
                let c = half(h);
                let d = c.point(1e-4) - c.start();
                out[origin(h)].push((d.y.atan2(d.x), h));
            }
        }
        for list in &mut out {
            list.sort_by(|a, b| a.0.total_cmp(&b.0));
        }
        // Arriving along h, leave by the edge just clockwise of h's twin.
        let next = |h: usize| -> usize {
            let twin = h ^ 1;
            let list = &out[origin(twin)];
            // Every live half-edge is listed at its origin.
            let Some(k) = list.iter().position(|&(_, x)| x == twin) else {
                return twin;
            };
            list[(k + list.len() - 1) % list.len()].1
        };

        // Trace the faces.
        let mut used = vec![false; 2 * edges.len()];
        let mut cycles: Vec<(Vec<usize>, Real)> = Vec::new();
        for h0 in (0..2 * edges.len()).filter(|h| alive[h / 2]) {
            if used[h0] {
                continue;
            }
            let mut cycle = Vec::new();
            let mut h = h0;
            while !used[h] {
                used[h] = true;
                cycle.push(h);
                h = next(h);
            }
            let poly: Vec<Point2> = cycle
                .iter()
                .flat_map(|&h| {
                    let pts = half(h).sample(1e-4 * scale);
                    pts[..pts.len() - 1].to_vec()
                })
                .collect();
            cycles.push((cycle, polygon_area(&poly)));
        }

        // Connected pieces (union–find over vertices).
        let mut parent: Vec<usize> = (0..nv).collect();
        fn find(p: &mut [usize], mut i: usize) -> usize {
            while p[i] != i {
                p[i] = p[p[i]];
                i = p[i];
            }
            i
        }
        for (i, e) in edges.iter().enumerate().filter(|(i, _)| alive[*i]) {
            let _ = i;
            let (a, b) = (find(&mut parent, e.from), find(&mut parent, e.to));
            parent[a] = b;
        }

        let to_loop = |cycle: &[usize]| ProfileLoop {
            edges: cycle
                .iter()
                .map(|&h| ProfileEdge {
                    source: edges[h / 2].source,
                    curve: half(h),
                })
                .collect(),
        };
        let min_area = 1e-12 * scale * scale;
        let faces: Vec<usize> = (0..cycles.len())
            .filter(|&i| cycles[i].1 > min_area)
            .collect();
        let mut regions: Vec<Region> = faces
            .iter()
            .map(|&i| Region {
                outer: to_loop(&cycles[i].0),
                holes: Vec::new(),
            })
            .collect();
        let polys: Vec<Vec<Point2>> = regions
            .iter()
            .map(|r| r.outer.polygon(1e-4 * scale))
            .collect();
        let comp_of = |parent: &mut Vec<usize>, cycle: &[usize]| find(parent, origin(cycle[0]));
        // Each piece's outer boundary is a hole of the smallest face of
        // another piece that contains it.
        for (cycle, area) in &cycles {
            if *area >= -min_area {
                continue;
            }
            let comp = comp_of(&mut parent, cycle);
            let probe = verts[origin(cycle[0])];
            let host = faces
                .iter()
                .enumerate()
                .filter(|&(_, &f)| comp_of(&mut parent, &cycles[f].0) != comp)
                .filter(|&(k, _)| point_in_polygon(&polys[k], probe))
                .min_by(|a, b| cycles[*a.1].1.total_cmp(&cycles[*b.1].1))
                .map(|(k, _)| k);
            if let Some(k) = host {
                regions[k].holes.push(to_loop(cycle));
            }
        }
        regions
    }

    /// The normal curves split at every crossing / touching point.
    fn profile_pieces(&self) -> Vec<(EntityId, ProfileCurve)> {
        let curves: Vec<(EntityId, &Entity)> = self
            .entities()
            // Centerlines are profile geometry too (Inventor): a half
            // section closed by its axis is a closed region.
            .filter(|(id, _)| self.style(*id) != Style::Construction)
            .collect();
        let geoms: Vec<Option<Geom>> = curves
            .iter()
            .map(|(id, _)| Geom::of(self, *id).ok())
            .collect();
        let pos = |p| self.point(p).map(|q| q.position);
        let ends: Vec<Point2> = curves
            .iter()
            .flat_map(|(_, e)| e.endpoints())
            .filter_map(pos)
            .collect();
        let mut out = Vec::new();
        for (i, (id, ent)) in curves.iter().enumerate() {
            let Some(g) = geoms[i] else {
                // Spline: whole, joined only at its end points.
                if let Entity::Spline { points } = ent {
                    if points.iter().any(|&p| pos(p).is_none()) {
                        continue;
                    }
                    // Control points and knots (computed for interpolation
                    // splines).
                    let (cv, knots) = self.spline_def(*id, &|p| {
                        self.point(p)
                            .map_or(Vector2::zeros(), |q| q.position.coords)
                    });
                    let points: Vec<Point2> = cv.into_iter().map(Point2::from).collect();
                    out.push((
                        *id,
                        ProfileCurve::Spline {
                            points,
                            reversed: false,
                            knots,
                        },
                    ));
                }
                continue;
            };
            let mut cuts: Vec<Real> = Vec::new();
            for (j, h) in geoms.iter().enumerate() {
                if let (Some(h), true) = (h, i != j) {
                    for p in g.carrier_hits(h) {
                        if g.contains(p) && h.contains(p) {
                            cuts.push(g.param(p));
                        }
                    }
                }
            }
            for &p in &ends {
                let on = match g {
                    Geom::Seg(a, b) => {
                        let d = b - a;
                        (d.perp(&(p - a)) / d.norm().max(1e-12)).abs() < 1e-9 * (1.0 + d.norm())
                    }
                    Geom::Arc { c, r, .. } | Geom::Circle { c, r } => {
                        ((p - c).norm() - r).abs() < 1e-9 * (1.0 + r)
                    }
                };
                if on && g.contains(p) {
                    cuts.push(g.param(p));
                }
            }
            cuts.sort_by(Real::total_cmp);
            cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
            match g {
                Geom::Seg(a, b) => {
                    let mut ts = vec![0.0];
                    ts.extend(cuts.iter().copied().filter(|&t| t > 1e-9 && t < 1.0 - 1e-9));
                    ts.push(1.0);
                    for w in ts.windows(2) {
                        let (p, q) = (a + (b - a) * w[0], a + (b - a) * w[1]);
                        out.push((*id, ProfileCurve::Line { a: p, b: q }));
                    }
                }
                Geom::Arc { c, r, a0, sweep } => {
                    let mut ts = vec![0.0];
                    ts.extend(
                        cuts.iter()
                            .copied()
                            .filter(|&t| t > 1e-9 && t < sweep - 1e-9),
                    );
                    ts.push(sweep);
                    for w in ts.windows(2) {
                        out.push((
                            *id,
                            ProfileCurve::Arc {
                                c,
                                r,
                                a0: a0 + w[0],
                                sweep: w[1] - w[0],
                            },
                        ));
                    }
                }
                Geom::Circle { c, r } => {
                    let ts: Vec<Real> = if cuts.is_empty() { vec![0.0] } else { cuts };
                    for (k, &t) in ts.iter().enumerate() {
                        let t1 = ts.get(k + 1).copied().unwrap_or(ts[0] + TAU);
                        out.push((
                            *id,
                            ProfileCurve::Arc {
                                c,
                                r,
                                a0: t,
                                sweep: t1 - t,
                            },
                        ));
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use std::f64::consts::PI;

    fn p(x: Real, y: Real) -> Point2 {
        Point2::new(x, y)
    }

    fn areas(s: &Sketch) -> Vec<Real> {
        let mut a: Vec<Real> = s.regions().iter().map(Region::area).collect();
        a.sort_by(|x, y| y.total_cmp(x));
        a
    }

    /// A circle drawn on another (a projected edge) and radial lines: one
    /// edge, 8 sectors (coincident pieces gave 0 regions).
    #[test]
    fn coincident_circles_are_one_edge() {
        let mut s = Sketch::default();
        let c = s.add_circle(p(0.0, 0.0), 20.0).unwrap();
        let Some(Entity::Circle { center, .. }) = s.entity(c).cloned() else {
            panic!("circle")
        };
        s.add_circle_at(center, 20.0).unwrap();
        for k in 0..8 {
            let a = std::f64::consts::TAU * k as Real / 8.0;
            let e = s.add_point(p(25.0 * a.cos(), 25.0 * a.sin()));
            s.add_line_between(center, e).unwrap();
        }
        let r = s.regions();
        assert_eq!(r.len(), 8);
        let total: Real = r.iter().map(Region::area).sum();
        assert!((total - std::f64::consts::PI * 400.0).abs() < 1e-3 * total);
    }

    /// A checkerboard of squares touching at corners inside a plate outline
    /// that shares their border edges: 16 cells of 100 (10 were found).
    #[test]
    fn checkerboard_on_a_plate_outline() {
        let mut s = Sketch::default();
        s.add_rectangle_two_point(p(0.0, 0.0), p(40.0, 40.0))
            .unwrap();
        for a in 0..4 {
            for b in 0..4 {
                if (a + b) % 2 == 0 {
                    let (x, y) = (a as Real * 10.0, b as Real * 10.0);
                    s.add_rectangle_two_point(p(x, y), p(x + 10.0, y + 10.0))
                        .unwrap();
                }
            }
        }
        let r = s.regions();
        assert_eq!(r.len(), 16);
        assert!(
            r.iter()
                .all(|r| (r.area() - 100.0).abs() < 1e-6 && r.holes.is_empty())
        );
    }

    #[test]
    fn plate_with_two_holes() {
        let mut s = Sketch::default();
        s.add_rectangle_two_point(p(0.0, 0.0), p(40.0, 20.0))
            .unwrap();
        s.add_circle(p(10.0, 10.0), 3.0).unwrap();
        s.add_circle(p(30.0, 10.0), 4.0).unwrap();
        let regions = s.regions();
        assert_eq!(regions.len(), 3);
        let plate = regions.iter().find(|r| r.holes.len() == 2).unwrap();
        assert_relative_eq!(plate.area(), 800.0 - PI * 25.0, epsilon = 1e-2);
        assert!(plate.contains(p(20.0, 10.0)));
        assert!(!plate.contains(p(10.0, 10.0)), "inside a hole");
        assert!(regions.iter().all(|r| r.contains(r.interior_point())));
        assert!(plate.outer.area() > 0.0 && plate.holes.iter().all(|h| h.area() < 0.0));
        assert_relative_eq!(areas(&s)[2], PI * 9.0, epsilon = 1e-2);
    }

    #[test]
    fn overlapping_rectangles_and_a_split_circle() {
        let mut s = Sketch::default();
        s.add_rectangle_two_point(p(0.0, 0.0), p(20.0, 10.0))
            .unwrap();
        s.add_rectangle_two_point(p(10.0, 5.0), p(30.0, 15.0))
            .unwrap();
        // 3 regions: two L-shapes and the overlap.
        let a = areas(&s);
        assert_eq!(a.len(), 3, "{a:?}");
        assert_relative_eq!(a.iter().sum::<Real>(), 350.0, epsilon = 1e-6);
        assert_relative_eq!(a[2], 50.0, epsilon = 1e-6);

        let mut s = Sketch::default();
        s.add_circle(p(0.0, 0.0), 5.0).unwrap();
        s.add_line(p(-10.0, 1.0), p(10.0, 1.0));
        let a = areas(&s);
        assert_eq!(a.len(), 2, "a chord splits the disc: {a:?}");
        assert_relative_eq!(a[0] + a[1], PI * 25.0, epsilon = 1e-2);
    }

    #[test]
    fn slot_spline_and_open_geometry() {
        let mut s = Sketch::default();
        s.add_slot_center_to_center(p(0.0, 0.0), p(10.0, 0.0), 4.0)
            .unwrap();
        assert_relative_eq!(areas(&s)[0], 40.0 + PI * 4.0, epsilon = 1e-2);

        // Open polyline and construction geometry give nothing; a dangling
        // line inside a region does not change it.
        let mut s = Sketch::default();
        let a = s.add_point(p(0.0, 0.0));
        let b = s.add_point(p(10.0, 0.0));
        let c = s.add_point(p(10.0, 10.0));
        s.add_line_between(a, b).unwrap();
        let bc = s.add_line_between(b, c).unwrap();
        assert!(s.regions().is_empty());
        let ca = s.add_line_between(c, a).unwrap();
        assert_relative_eq!(areas(&s)[0], 50.0, epsilon = 1e-9);
        s.add_line(p(5.0, 1.0), p(8.0, 1.5));
        assert_eq!(areas(&s), vec![50.0]);
        s.set_style(ca, Style::Construction).unwrap();
        assert!(s.regions().is_empty());
        s.set_style(ca, Style::Normal).unwrap();

        // A spline closing a line: one region.
        let mut s = Sketch::default();
        let a = s.add_point(p(0.0, 0.0));
        let b = s.add_point(p(10.0, 0.0));
        let m1 = s.add_point(p(10.0, 8.0));
        let m2 = s.add_point(p(0.0, 8.0));
        s.add_line_between(a, b).unwrap();
        s.add_spline(vec![b, m1, m2, a]).unwrap();
        let r = s.regions();
        assert_eq!(r.len(), 1);
        assert!(r[0].area() > 30.0);
        let _ = bc;
    }
}
