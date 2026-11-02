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

/// Do two edges run along the same path, in either direction?
fn same_path(a: &ProfileCurve, b: &ProfileCurve, tol: Real) -> bool {
    let close = |p: Point2, q: Point2| (p - q).norm() <= tol;
    let forward = close(a.start(), b.start())
        && close(a.end(), b.end())
        && close(a.point(0.25), b.point(0.25));
    let backward = close(a.start(), b.end())
        && close(a.end(), b.start())
        && close(a.point(0.25), b.point(0.75));
    (forward || backward) && close(a.point(0.5), b.point(0.5))
}

/// Joins the regions among `regions` that share an edge into one region each:
/// the shared edges disappear (Inventor: selected profiles that touch make one
/// body, without walls between them). A rectangle with two holes and the two
/// discs filling them becomes the whole rectangle. The result is ordered by
/// each group's first region and gives that first index; a region sharing no
/// edge comes back unchanged, as does a group whose outline does not make one
/// outer loop (regions meeting only at corners around a gap).
pub fn merge_regions(regions: &[&Region]) -> Vec<(usize, Region)> {
    let tol = 1e-6
        * regions
            .iter()
            .flat_map(|r| r.outer.edges.iter())
            .flat_map(|e| [e.curve.start(), e.curve.end()])
            .map(|p| p.coords.amax())
            .fold(1.0, Real::max);
    let all: Vec<Vec<&ProfileEdge>> = regions
        .iter()
        .map(|r| {
            std::iter::once(&r.outer)
                .chain(r.holes.iter())
                .flat_map(|l| l.edges.iter())
                .collect()
        })
        .collect();
    // Which regions touch: union-find over shared edges.
    let mut group: Vec<usize> = (0..regions.len()).collect();
    fn root(g: &mut [usize], mut i: usize) -> usize {
        while g[i] != i {
            g[i] = g[g[i]];
            i = g[i];
        }
        i
    }
    for i in 0..regions.len() {
        for j in i + 1..regions.len() {
            let shares = all[i]
                .iter()
                .any(|a| all[j].iter().any(|b| same_path(&a.curve, &b.curve, tol)));
            if shares {
                let (a, b) = (root(&mut group, i), root(&mut group, j));
                group[a.max(b)] = a.min(b);
            }
        }
    }
    let mut out = Vec::new();
    for i in 0..regions.len() {
        if root(&mut group, i) != i {
            continue;
        }
        let members: Vec<usize> = (0..regions.len())
            .filter(|&k| root(&mut group, k) == i)
            .collect();
        if members.len() == 1 {
            out.push((i, regions[i].clone()));
            continue;
        }
        // An edge stays when no other member of the group has it.
        let mut edges: Vec<&ProfileEdge> = Vec::new();
        for &k in &members {
            for e in &all[k] {
                let shared = members
                    .iter()
                    .filter(|&&o| o != k)
                    .any(|&o| all[o].iter().any(|b| same_path(&e.curve, &b.curve, tol)));
                if !shared {
                    edges.push(e);
                }
            }
        }
        match region_of_edges(&edges, tol) {
            Some(r) => out.push((i, r)),
            None => out.extend(members.iter().map(|&k| (k, regions[k].clone()))),
        }
    }
    out.sort_by_key(|(k, _)| *k);
    out
}

/// The region bounded by `edges` (each runs with the material on its left):
/// chained head to tail, exactly one counter-clockwise loop. `None` otherwise.
fn region_of_edges(edges: &[&ProfileEdge], tol: Real) -> Option<Region> {
    let mut used = vec![false; edges.len()];
    let mut outer: Option<ProfileLoop> = None;
    let mut holes = Vec::new();
    while let Some(first) = used.iter().position(|u| !u) {
        used[first] = true;
        let start = edges[first].curve.start();
        let mut end = edges[first].curve.end();
        let mut chain = vec![edges[first].clone()];
        while (end - start).norm() > tol {
            let next = (0..edges.len())
                .find(|&k| !used[k] && (edges[k].curve.start() - end).norm() <= tol)?;
            used[next] = true;
            end = edges[next].curve.end();
            chain.push(edges[next].clone());
        }
        let lp = ProfileLoop { edges: chain };
        if lp.area() > 0.0 {
            if outer.is_some() {
                return None;
            }
            outer = Some(lp);
        } else {
            holes.push(lp);
        }
    }
    Some(Region {
        outer: outer?,
        holes,
    })
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
        let scale = ends.iter().map(|p| p.coords.amax()).fold(1.0, Real::max);
        // Splines meet the other curves where the samples say so: both are cut there.
        let mut walks: Vec<Option<SplineWalk>> = curves
            .iter()
            .map(|(id, ent)| match ent {
                Entity::Spline { points } if points.iter().all(|&p| pos(p).is_some()) => {
                    let (cv, knots) = self.spline_def(*id, &|p| {
                        self.point(p)
                            .map_or(Vector2::zeros(), |q| q.position.coords)
                    });
                    SplineWalk::new(cv.into_iter().map(Point2::from).collect(), &knots)
                }
                _ => None,
            })
            .collect();
        let mut extra: Vec<Vec<Point2>> = vec![Vec::new(); curves.len()];
        for i in 0..curves.len() {
            if walks[i].is_none() {
                continue;
            }
            for (j, h) in geoms.iter().enumerate() {
                let Some(h) = h else {
                    continue;
                };
                let hits = walks[i].as_ref().map_or_else(Vec::new, |w| w.hits_with(h));
                for (u, x) in hits {
                    extra[j].push(x);
                    if let Some(w) = walks[i].as_mut() {
                        w.cut(u, scale);
                    }
                }
            }
            for j in i + 1..curves.len() {
                let hits = match (&walks[i], &walks[j]) {
                    (Some(a), Some(b)) => a.hits_with_spline(b),
                    _ => Vec::new(),
                };
                for (u, v) in hits {
                    if let Some(w) = walks[i].as_mut() {
                        w.cut(u, scale);
                    }
                    if let Some(w) = walks[j].as_mut() {
                        w.cut(v, scale);
                    }
                }
            }
            for &q in &ends {
                let at = walks[i].as_ref().and_then(|w| w.on_curve(q, scale));
                if let (Some(u), Some(w)) = (at, walks[i].as_mut()) {
                    w.cut(u, scale);
                }
            }
        }
        let mut out = Vec::new();
        for (i, (id, ent)) in curves.iter().enumerate() {
            let Some(g) = geoms[i] else {
                // A spline met by another curve: a piece between each two meetings.
                if let Some(w) = walks[i].as_ref().filter(|w| !w.cuts.is_empty()) {
                    for piece in w.pieces() {
                        out.push((*id, piece));
                    }
                    continue;
                }
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
            let mut cuts: Vec<Real> = extra[i].iter().map(|p| g.param(*p)).collect();
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

/// A spline's samples, to find where other curves meet it, and the places found.
struct SplineWalk {
    curve: crate::kernel::BSplineCurve,
    /// (parameter, point) along the curve.
    samples: Vec<(Real, Point2)>,
    /// Parameters to cut at, sorted, none at the ends.
    cuts: Vec<Real>,
}

impl SplineWalk {
    fn new(points: Vec<Point2>, knots: &[Real]) -> Option<Self> {
        use crate::kernel::{BSplineCurve, Point3};
        let n = points.len();
        let cvs = points.iter().map(|p| Point3::new(p.x, p.y, 0.0)).collect();
        let curve = BSplineCurve::sketch_spline(cvs, knots).ok()?;
        let d = curve.domain();
        let m = (32 * n).max(64);
        let mut w = Self {
            curve,
            samples: Vec::with_capacity(m + 1),
            cuts: Vec::new(),
        };
        for k in 0..=m {
            let u = d.min + (d.max - d.min) * k as Real / m as Real;
            let q = w.at(u);
            w.samples.push((u, q));
        }
        Some(w)
    }

    fn at(&self, u: Real) -> Point2 {
        let q = self.curve.point_at(u);
        Point2::new(q.x, q.y)
    }

    /// Where the curve crosses `g`: (parameter, point on the curve).
    fn hits_with(&self, g: &Geom) -> Vec<(Real, Point2)> {
        let f = |p: Point2| match *g {
            Geom::Seg(a, b) => {
                let d = b - a;
                d.perp(&(p - a)) / d.norm().max(1e-12)
            }
            Geom::Arc { c, r, .. } | Geom::Circle { c, r } => (p - c).norm() - r,
        };
        let mut out = Vec::new();
        for w in self.samples.windows(2) {
            let (mut lo, mut hi) = (w[0].0, w[1].0);
            let (mut flo, fhi) = (f(w[0].1), f(w[1].1));
            // Along the other curve (a chain's own joint) is no crossing.
            if flo * fhi >= 0.0 || (flo.abs() < 1e-9 && fhi.abs() < 1e-9) {
                continue;
            }
            for _ in 0..48 {
                let mid = (lo + hi) / 2.0;
                let fm = f(self.at(mid));
                if flo * fm <= 0.0 {
                    hi = mid;
                } else {
                    lo = mid;
                    flo = fm;
                }
            }
            let u = (lo + hi) / 2.0;
            let x = self.at(u);
            if g.contains(x) {
                out.push((u, x));
            }
        }
        out
    }

    /// Where two splines cross: (this one's parameter, the other's).
    fn hits_with_spline(&self, o: &SplineWalk) -> Vec<(Real, Real)> {
        let mut out = Vec::new();
        for a in self.samples.windows(2) {
            let (p, r) = (a[0].1, a[1].1 - a[0].1);
            for b in o.samples.windows(2) {
                let (q, s) = (b[0].1, b[1].1 - b[0].1);
                let den = r.perp(&s);
                if den.abs() < 1e-12 {
                    continue;
                }
                let t = (q - p).perp(&s) / den;
                let v = (q - p).perp(&r) / den;
                if !(0.0..=1.0).contains(&t) || !(0.0..=1.0).contains(&v) {
                    continue;
                }
                // Newton from the samples' crossing, so that both end on one point.
                let mut u0 = a[0].0 + (a[1].0 - a[0].0) * t;
                let mut u1 = b[0].0 + (b[1].0 - b[0].0) * v;
                for _ in 0..8 {
                    let h = 1e-6 * (a[1].0 - a[0].0).abs().max(1e-9);
                    let e = self.at(u0) - o.at(u1);
                    let d0 = (self.at(u0 + h) - self.at(u0 - h)) / (2.0 * h);
                    let d1 = (o.at(u1 + h) - o.at(u1 - h)) / (2.0 * h);
                    let det = d1.perp(&d0);
                    if det.abs() < 1e-14 {
                        break;
                    }
                    u0 -= d1.perp(&e) / det;
                    u1 -= d0.perp(&e) / det;
                }
                out.push((u0, u1));
            }
        }
        out
    }

    /// The parameter of `q` when it lies on the curve (a line ending on it).
    fn on_curve(&self, q: Point2, scale: Real) -> Option<Real> {
        let (k, _) = self
            .samples
            .iter()
            .enumerate()
            .min_by(|a, b| (a.1.1 - q).norm().total_cmp(&(b.1.1 - q).norm()))?;
        let (mut lo, mut hi) = (
            self.samples[k.saturating_sub(1)].0,
            self.samples[(k + 1).min(self.samples.len() - 1)].0,
        );
        for _ in 0..60 {
            let (m1, m2) = (lo + (hi - lo) / 3.0, hi - (hi - lo) / 3.0);
            if (self.at(m1) - q).norm() < (self.at(m2) - q).norm() {
                hi = m2;
            } else {
                lo = m1;
            }
        }
        let u = (lo + hi) / 2.0;
        ((self.at(u) - q).norm() <= 1e-7 * scale).then_some(u)
    }

    /// Cuts at `u`, unless it is at an end of the curve or at a cut already there.
    fn cut(&mut self, u: Real, scale: Real) {
        let d = self.curve.domain();
        let x = self.at(u);
        let tol = 1e-6 * scale;
        let near = |v: Real, s: &Self| (s.at(v) - x).norm() <= tol;
        if near(d.min, self) || near(d.max, self) || self.cuts.iter().any(|v| near(*v, self)) {
            return;
        }
        self.cuts.push(u);
        self.cuts.sort_by(Real::total_cmp);
    }

    /// The curve between its cuts: splines of their own (full knots at each cut).
    fn pieces(&self) -> Vec<ProfileCurve> {
        let p = self.curve.degree();
        let to2 = |c: &crate::kernel::Point3| Point2::new(c.x, c.y);
        let mut cv: Vec<Point2> = self.curve.control_points().iter().map(to2).collect();
        let mut knots: Vec<Real> = self.curve.knots().to_vec();
        let mut out = Vec::new();
        for &u in &self.cuts {
            let (left_cv, left_k, right_cv, right_k) = split_bspline(&cv, &knots, p, u);
            out.push(ProfileCurve::Spline {
                points: left_cv,
                reversed: false,
                knots: left_k,
            });
            cv = right_cv;
            knots = right_k;
        }
        out.push(ProfileCurve::Spline {
            points: cv,
            reversed: false,
            knots,
        });
        out
    }
}

/// A clamped B-spline cut at `u` by knot insertion: the two curves it is made of.
pub(super) fn split_bspline(
    cv: &[Point2],
    knots: &[Real],
    p: usize,
    u: Real,
) -> (Vec<Point2>, Vec<Real>, Vec<Point2>, Vec<Real>) {
    let mut cv = cv.to_vec();
    let mut knots = knots.to_vec();
    let eps = 1e-9 * (knots[knots.len() - 1] - knots[0]).abs().max(1e-12);
    // A cut at a knot already there uses it.
    let u = knots
        .iter()
        .copied()
        .find(|k| (k - u).abs() <= eps)
        .unwrap_or(u);
    while knots.iter().filter(|k| (*k - u).abs() <= eps).count() < p {
        // The span holding u: knots[k] <= u < knots[k + 1].
        let k = knots
            .iter()
            .rposition(|x| *x <= u + eps)
            .unwrap_or(0)
            .min(knots.len() - 2);
        let mut next = Vec::with_capacity(cv.len() + 1);
        for i in 0..=cv.len() {
            if i + p <= k {
                next.push(cv[i]);
            } else if i > k {
                next.push(cv[i - 1]);
            } else {
                let a = (u - knots[i]) / (knots[i + p] - knots[i]);
                next.push(Point2::from(
                    cv[i - 1].coords * (1.0 - a) + cv[i].coords * a,
                ));
            }
        }
        cv = next;
        knots.insert(k + 1, u);
    }
    let j = knots.iter().position(|x| (x - u).abs() <= eps).unwrap_or(p);
    let mut left_k = knots[..j + p].to_vec();
    left_k.push(u);
    let mut right_k = vec![u];
    right_k.extend_from_slice(&knots[j..]);
    (cv[..j].to_vec(), left_k, cv[j - 1..].to_vec(), right_k)
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
    fn touching_regions_merge_into_one() {
        // A plate with two holes and the discs filling them: one rectangle.
        let mut s = Sketch::default();
        s.add_rectangle_two_point(p(-6.0, -20.0), p(6.0, 20.0))
            .unwrap();
        s.add_circle(p(0.0, 15.0), 3.0).unwrap();
        s.add_circle(p(0.0, -15.0), 3.0).unwrap();
        let regions = s.regions();
        assert_eq!(regions.len(), 3);
        let refs: Vec<&Region> = regions.iter().collect();
        let merged = merge_regions(&refs);
        assert_eq!(merged.len(), 1);
        assert!(merged[0].1.holes.is_empty());
        assert_relative_eq!(merged[0].1.area(), 480.0, epsilon = 1e-6);

        // The plate and one disc: the other hole stays.
        let plate = regions.iter().find(|r| r.holes.len() == 2).unwrap();
        let disc = regions.iter().find(|r| r.holes.is_empty()).unwrap();
        let merged = merge_regions(&[plate, disc]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].1.holes.len(), 1);

        // Two discs apart stay two regions.
        let discs: Vec<&Region> = regions.iter().filter(|r| r.holes.is_empty()).collect();
        assert_eq!(merge_regions(&discs).len(), 2);
    }

    #[test]
    fn neighbours_across_a_shared_line_merge() {
        // A rectangle split by a line, with a circle over that line.
        let mut s = Sketch::default();
        s.add_rectangle_two_point(p(0.0, 0.0), p(20.0, 10.0))
            .unwrap();
        s.add_line(p(10.0, 0.0), p(10.0, 10.0));
        s.add_circle(p(10.0, 5.0), 2.0).unwrap();
        let regions = s.regions();
        assert!(regions.len() >= 4, "{}", regions.len());
        let refs: Vec<&Region> = regions.iter().collect();
        let merged = merge_regions(&refs);
        assert_eq!(merged.len(), 1);
        assert_relative_eq!(merged[0].1.area(), 200.0, epsilon = 1e-3);
        assert!(merged[0].1.holes.is_empty());
    }

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

#[cfg(test)]
mod arc_in_a_rectangle {
    use super::super::*;

    /// A fully fixed rectangle 100 × 40, an arc from its lower right corner ending on the top
    /// line. Returns the sketch, the arc, its end, centre and the lines (bottom, right, top).
    fn setup() -> (Sketch, EntityId, PointId, PointId, [EntityId; 3]) {
        let mut s = Sketch::default();
        let sh = s
            .add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(100.0, 40.0))
            .unwrap();
        for p in &sh.points {
            s.set_fixed(*p, true).unwrap();
        }
        let end = s.add_point(Point2::new(70.0, 38.0));
        let c = s.add_point(Point2::new(68.0, 10.0));
        let arc = s.add_arc(c, sh.points[1], end).unwrap();
        (
            s,
            arc,
            end,
            c,
            [sh.entities[0], sh.entities[1], sh.entities[2]],
        )
    }

    fn solved_clean(s: &mut Sketch, what: &str) -> SolveReport {
        let r = s.solve().unwrap();
        assert_eq!(r.status, SolveStatus::Solved, "{what}: {r:?}");
        // 4: the fixed corners repeat the rectangle's own Horizontal / Vertical.
        assert_eq!(r.redundant, 4, "{what}: {r:?}");
        r
    }

    /// The owner's arc: its end on the top line and tangent to it, in either order.
    #[test]
    fn its_end_on_a_line_and_tangent_to_that_line() {
        for order in [0, 1] {
            let (mut s, arc, end, c, [_, _, top]) = setup();
            let on = Constraint::PointOnLine {
                point: end,
                line: top,
            };
            if order == 0 {
                s.add_constraint(on).unwrap();
                solved_clean(&mut s, "on");
                s.add_tangent(arc, top).unwrap();
            } else {
                s.add_tangent(arc, top).unwrap();
                solved_clean(&mut s, "tangent");
                s.add_constraint(on).unwrap();
            }
            let r = solved_clean(&mut s, "both");
            assert_eq!(r.dof, 1, "one size left: {r:?}");
            let (e, q) = (s.point(end).unwrap().position, s.point(c).unwrap().position);
            assert!((e.y - 40.0).abs() < 1e-6, "{e:?}");
            assert!(
                (e.x - q.x).abs() < 1e-6,
                "the radius is square to the line: {e:?} {q:?}"
            );
        }
    }

    /// The same end on the top line and the arc tangent at the corner, to the right or the
    /// bottom line: either side of the corner.
    #[test]
    fn tangent_at_the_corner_and_its_end_on_the_top_line() {
        for which in [0, 1] {
            let (mut s, arc, end, c, lines) = setup();
            s.add_constraint(Constraint::PointOnLine {
                point: end,
                line: lines[2],
            })
            .unwrap();
            s.add_tangent(arc, lines[which]).unwrap();
            let r = solved_clean(&mut s, "corner");
            assert_eq!(r.dof, 1, "{which}: {r:?}");
            let q = s.point(c).unwrap().position;
            if which == 0 {
                assert!((q.x - 100.0).abs() < 1e-6, "{q:?}");
            } else {
                assert!(q.y.abs() < 1e-6, "{q:?}");
            }
        }
    }

    /// Tangent at the corner, its end on the top line, then tangent to the top line too: a
    /// half circle, a long way from where the arc stands.
    #[test]
    fn two_tangents_and_its_end_on_the_line_reach_a_half_circle() {
        let (mut s, arc, end, c, lines) = setup();
        s.add_constraint(Constraint::PointOnLine {
            point: end,
            line: lines[2],
        })
        .unwrap();
        s.add_tangent(arc, lines[0]).unwrap();
        solved_clean(&mut s, "first tangent");
        s.add_tangent(arc, lines[2]).unwrap();
        let r = s.solve().unwrap();
        println!(
            "{r:?} c={:?} e={:?}",
            s.point(c).unwrap().position,
            s.point(end).unwrap().position
        );
        solved_clean(&mut s, "second tangent");
        let (e, q) = (s.point(end).unwrap().position, s.point(c).unwrap().position);
        assert!((e - Point2::new(100.0, 40.0)).norm() < 1e-6, "{e:?}");
        assert!((q - Point2::new(100.0, 20.0)).norm() < 1e-6, "{q:?}");
    }

    /// Fully defined by its radius (the arc of the owner's blade).
    #[test]
    fn a_radius_finishes_it() {
        let (mut s, arc, end, _, [_, _, top]) = setup();
        s.add_constraint(Constraint::PointOnLine {
            point: end,
            line: top,
        })
        .unwrap();
        s.add_tangent(arc, top).unwrap();
        s.add_constraint(Constraint::Radius {
            circle: arc,
            value: 45.0,
        })
        .unwrap();
        let r = solved_clean(&mut s, "radius");
        assert_eq!(r.dof, 0, "{r:?}");
    }
}

#[cfg(test)]
mod arc_from_far {
    use super::super::*;

    /// The same arc as drawn in the view (its centre almost on the top line, so its radius at
    /// the end lies along that line): a cosine residual had no slope there and the second
    /// tangent was refused as a conflict.
    #[test]
    fn a_second_tangent_from_where_the_arc_was_drawn() {
        let mut s = Sketch::default();
        let sh = s
            .add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(40.0, 25.0))
            .unwrap();
        s.set_fixed(sh.points[0], true).unwrap();
        for (a, b, value) in [(0, 1, 40.0), (1, 2, 25.0)] {
            s.add_constraint(Constraint::Distance {
                a: sh.points[a],
                b: sh.points[b],
                value,
            })
            .unwrap();
        }
        let c = s.add_point(Point2::new(40.0, 25.4));
        let e = s.add_point(Point2::new(14.6, 25.0));
        let arc = s.add_arc(c, sh.points[1], e).unwrap();
        s.add_tangent(arc, sh.entities[0]).unwrap();
        s.add_constraint(Constraint::PointOnLine {
            point: e,
            line: sh.entities[2],
        })
        .unwrap();
        s.add_tangent(arc, sh.entities[2]).unwrap();
        let r = s.solve().unwrap();
        assert_eq!(
            (r.status, r.dof, r.redundant),
            (SolveStatus::Solved, 0, 0),
            "{r:?}"
        );
        let (q, p) = (s.point(c).unwrap().position, s.point(e).unwrap().position);
        assert!((q - Point2::new(40.0, 12.5)).norm() < 1e-6, "{q:?}");
        assert!((p - Point2::new(40.0, 25.0)).norm() < 1e-6, "{p:?}");
    }
}

#[cfg(test)]
mod splines_divide {
    use super::super::*;

    fn p(x: Real, y: Real) -> Point2 {
        Point2::new(x, y)
    }

    fn closed_spline(s: &mut Sketch, at: &[(Real, Real)]) -> EntityId {
        let ids: Vec<_> = at.iter().map(|&(x, y)| s.add_point(p(x, y))).collect();
        let mut l = ids.clone();
        l.push(ids[0]);
        s.add_interp_spline(l).unwrap()
    }

    /// Area of a shape from fine samples of its region list.
    fn total(r: &[Region]) -> Real {
        r.iter().map(Region::area).sum()
    }

    #[test]
    fn a_closed_spline_across_a_rectangle_makes_three_regions() {
        let mut alone = Sketch::default();
        closed_spline(
            &mut alone,
            &[(5.0, -10.0), (15.0, -5.0), (17.0, 8.0), (8.0, 12.0)],
        );
        let whole = total(&alone.regions());
        let mut s = Sketch::default();
        s.add_rectangle_two_point(p(0.0, 0.0), p(40.0, 25.0))
            .unwrap();
        closed_spline(
            &mut s,
            &[(5.0, -10.0), (15.0, -5.0), (17.0, 8.0), (8.0, 12.0)],
        );
        let r = s.regions();
        assert_eq!(
            r.len(),
            3,
            "{:?}",
            r.iter().map(Region::area).collect::<Vec<_>>()
        );
        // The rectangle's bite + the lens + what sticks out: the two shapes, the lens once.
        let lens = r
            .iter()
            .map(Region::area)
            .find(|a| *a > 1.0 && *a < whole)
            .unwrap_or(0.0);
        assert!(total(&r) < 1000.0 + whole, "{}", total(&r));
        assert!(total(&r) > 1000.0, "{lens}");
        // Touching regions come back as one outline, the rectangle and the spline's outside.
        let refs: Vec<&Region> = r.iter().collect();
        let m = merge_regions(&refs);
        assert_eq!(m.len(), 1);
        assert!(
            (m[0].1.area() - total(&r)).abs() < 1e-3,
            "{}",
            m[0].1.area()
        );
    }

    #[test]
    fn split_pieces_keep_the_curve() {
        let cv = [
            p(0.0, 0.0),
            p(1.0, 3.0),
            p(4.0, 3.0),
            p(6.0, 0.0),
            p(9.0, 1.0),
        ];
        let w = SplineWalkProbe::new(&cv);
        for u in [0.21, 0.5, 0.77] {
            let (a, ak, b, bk) = w.split(u);
            let at = w.at(u);
            assert!((a[a.len() - 1] - at).norm() < 1e-9, "left end {u}");
            assert!((b[0] - at).norm() < 1e-9, "right start {u}");
            for s in [0.25, 0.5, 0.75] {
                let v = u * s;
                let q = SplineWalkProbe::of(&a, &ak).at_u(v);
                assert!((q - w.at(v)).norm() < 1e-9, "left at {v}");
                let v = u + (1.0 - u) * s;
                let q = SplineWalkProbe::of(&b, &bk).at_u(v);
                assert!((q - w.at(v)).norm() < 1e-9, "right at {v}");
            }
        }
    }

    /// A curve on [0, 1] to check the split against.
    struct SplineWalkProbe(crate::kernel::BSplineCurve);

    impl SplineWalkProbe {
        fn of(cv: &[Point2], knots: &[Real]) -> Self {
            let cvs = cv
                .iter()
                .map(|q| crate::kernel::Point3::new(q.x, q.y, 0.0))
                .collect();
            Self(crate::kernel::BSplineCurve::sketch_spline(cvs, knots).unwrap())
        }

        fn new(cv: &[Point2]) -> Self {
            Self::of(cv, &[0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0])
        }

        fn at_u(&self, u: Real) -> Point2 {
            let q = self.0.point_at(u);
            p(q.x, q.y)
        }

        fn at(&self, u: Real) -> Point2 {
            self.at_u(u)
        }

        fn split(&self, u: Real) -> (Vec<Point2>, Vec<Real>, Vec<Point2>, Vec<Real>) {
            let cv: Vec<Point2> = self
                .0
                .control_points()
                .iter()
                .map(|c| p(c.x, c.y))
                .collect();
            super::super::profile::split_bspline(&cv, self.0.knots(), self.0.degree(), u)
        }
    }

    #[test]
    fn two_splines_crossing_and_a_line_ending_on_a_spline() {
        let mut s = Sketch::default();
        closed_spline(
            &mut s,
            &[(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)],
        );
        closed_spline(
            &mut s,
            &[(10.0, 5.0), (30.0, 5.0), (30.0, 25.0), (10.0, 25.0)],
        );
        assert_eq!(s.regions().len(), 3);
        // A line across a closed spline's inside, ending on it at both ends.
        let mut l = Sketch::default();
        let e = closed_spline(
            &mut l,
            &[(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)],
        );
        let a = l.add_point(p(10.0, -5.0));
        let b = l.add_point(p(10.0, 25.0));
        l.add_line_between(a, b).unwrap();
        let r = l.regions();
        assert_eq!(
            r.len(),
            2,
            "{:?}",
            r.iter().map(Region::area).collect::<Vec<_>>()
        );
        let _ = e;
    }

    #[test]
    fn a_spline_touching_nothing_stays_whole() {
        let mut s = Sketch::default();
        s.add_rectangle_two_point(p(0.0, 0.0), p(10.0, 10.0))
            .unwrap();
        closed_spline(
            &mut s,
            &[(20.0, 0.0), (30.0, 0.0), (30.0, 10.0), (20.0, 10.0)],
        );
        let r = s.regions();
        assert_eq!(r.len(), 2);
        assert!(r.iter().any(|x| x.outer.edges.len() == 1));
    }
}
