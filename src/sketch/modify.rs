//! Modify tools of the Sketch tab (Inventor: Sketch → Modify): Trim, Extend,
//! Split, Offset, Move, Copy, Rotate, Mirror.
//!
//! Behaviour follows Inventor:
//!
//! - **Trim** removes the clicked part of a curve up to the nearest
//!   intersecting curves (the whole curve if nothing intersects it); new end
//!   points get a Coincident (point-on-curve) constraint to the boundary.
//! - **Extend** lengthens the end nearest to the click to the nearest curve
//!   in that direction, again with a Coincident to the boundary.
//! - **Split** cuts a curve at the intersection nearest to the click; both
//!   pieces stay on the original carrier (Collinear lines / equal concentric
//!   arcs), which preserves the original's H/V/∥/⟂/collinear relations.
//! - **Offset** copies a curve — or with *Loop Select* the whole chain of
//!   curves joined at their endpoints — at a distance; lines stay parallel,
//!   arcs concentric, and with *Constrain Offset* all pieces keep one common
//!   offset (one DOF left: the distance, to be dimensioned).
//! - **Move/Rotate** move the geometry (the solver then restores the
//!   constraints); **Copy** duplicates it with its internal constraints;
//!   **Mirror** creates a mirror image tied by Symmetric constraints.
//!
//! Splines are not trimmed/extended/split/offset yet.

use std::collections::HashMap;
use std::f64::consts::TAU;

use super::{Constraint, Entity, EntityId, Point2, PointId, Shape, Sketch, SketchError, Vector2};
use crate::kernel::Real;

const EPS: Real = 1e-7;

/// Geometry of a curve, for intersection maths.
#[derive(Clone, Copy, Debug)]
pub(super) enum Geom {
    /// Segment `a → b`; parameter t ∈ [0, 1].
    Seg(Point2, Point2),
    /// Arc counter-clockwise from angle `a0` over `sweep`; parameter is the
    /// angle from the start, in [0, sweep].
    Arc {
        c: Point2,
        r: Real,
        a0: Real,
        sweep: Real,
    },
    /// Full circle; parameter is the absolute angle in [0, 2π).
    Circle { c: Point2, r: Real },
}

fn angle_of(c: Point2, p: Point2) -> Real {
    (p.y - c.y).atan2(p.x - c.x)
}

fn norm_angle(a: Real) -> Real {
    a.rem_euclid(TAU)
}

impl Geom {
    pub(super) fn of(sk: &Sketch, e: EntityId) -> Result<Geom, SketchError> {
        let pos = |p: PointId| {
            sk.point(p)
                .map(|q| q.position)
                .ok_or(SketchError::UnknownPoint)
        };
        match sk.entity(e).ok_or(SketchError::UnknownEntity)? {
            Entity::Line { start, end } => Ok(Geom::Seg(pos(*start)?, pos(*end)?)),
            Entity::Circle { center, radius } => Ok(Geom::Circle {
                c: pos(*center)?,
                r: *radius,
            }),
            Entity::Arc { center, start, end } => {
                let (c, s, e) = (pos(*center)?, pos(*start)?, pos(*end)?);
                let a0 = angle_of(c, s);
                let mut sweep = norm_angle(angle_of(c, e) - a0);
                if sweep < EPS {
                    sweep = TAU;
                }
                Ok(Geom::Arc {
                    c,
                    r: (s - c).norm(),
                    a0,
                    sweep,
                })
            }
            Entity::Spline { .. } => Err(SketchError::InvalidValue(
                "splines cannot be trimmed, extended, split or offset yet",
            )),
        }
    }

    /// Parameter of `p` on the carrier (line or circle), not clamped.
    pub(super) fn param(&self, p: Point2) -> Real {
        match *self {
            Geom::Seg(a, b) => (p - a).dot(&(b - a)) / (b - a).norm_squared().max(1e-24),
            Geom::Arc { c, a0, .. } => norm_angle(angle_of(c, p) - a0),
            Geom::Circle { c, .. } => norm_angle(angle_of(c, p)),
        }
    }

    pub(super) fn point(&self, t: Real) -> Point2 {
        match *self {
            Geom::Seg(a, b) => a + (b - a) * t,
            Geom::Arc { c, r, a0, .. } => c + Vector2::new((a0 + t).cos(), (a0 + t).sin()) * r,
            Geom::Circle { c, r } => c + Vector2::new(t.cos(), t.sin()) * r,
        }
    }

    /// Is `p` (on the carrier) within the curve's extent?
    pub(super) fn contains(&self, p: Point2) -> bool {
        match *self {
            Geom::Seg(..) => (-EPS..=1.0 + EPS).contains(&self.param(p)),
            Geom::Arc { sweep, .. } => {
                let t = self.param(p);
                t <= sweep + EPS || t >= TAU - EPS
            }
            Geom::Circle { .. } => true,
        }
    }

    /// Intersection points of the two carriers (infinite line / full circle).
    pub(super) fn carrier_hits(&self, other: &Geom) -> Vec<Point2> {
        let circle = |g: &Geom| match *g {
            Geom::Arc { c, r, .. } | Geom::Circle { c, r } => Some((c, r)),
            Geom::Seg(..) => None,
        };
        match (self, other) {
            (Geom::Seg(a, b), Geom::Seg(c, d)) => {
                let (r, s) = (b - a, d - c);
                let den = r.perp(&s);
                if den.abs() < 1e-12 {
                    return Vec::new();
                }
                vec![a + r * ((c - a).perp(&s) / den)]
            }
            (Geom::Seg(a, b), g) | (g, Geom::Seg(a, b)) => {
                let Some((c, r)) = circle(g) else {
                    return Vec::new();
                };
                line_circle(*a, *b, c, r)
            }
            (g, h) => {
                let (Some((c1, r1)), Some((c2, r2))) = (circle(g), circle(h)) else {
                    return Vec::new();
                };
                circle_circle(c1, r1, c2, r2)
            }
        }
    }
}

fn line_circle(a: Point2, b: Point2, c: Point2, r: Real) -> Vec<Point2> {
    let d = (b - a).normalize();
    let f = a - c;
    let bq = f.dot(&d);
    let disc = bq * bq - (f.norm_squared() - r * r);
    if disc < -1e-12 {
        return Vec::new();
    }
    let s = disc.max(0.0).sqrt();
    if s < 1e-12 {
        vec![a + d * (-bq)]
    } else {
        vec![a + d * (-bq - s), a + d * (-bq + s)]
    }
}

fn circle_circle(c1: Point2, r1: Real, c2: Point2, r2: Real) -> Vec<Point2> {
    let d = (c2 - c1).norm();
    if d < 1e-12 || d > r1 + r2 + 1e-9 || d < (r1 - r2).abs() - 1e-9 {
        return Vec::new();
    }
    let a = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
    let h = (r1 * r1 - a * a).max(0.0).sqrt();
    let u = (c2 - c1) / d;
    let m = c1 + u * a;
    let n = Vector2::new(-u.y, u.x);
    if h < 1e-12 {
        vec![m]
    } else {
        vec![m + n * h, m - n * h]
    }
}

/// A place where a curve meets another one.
#[derive(Clone, Copy, Debug)]
struct Hit {
    t: Real,
    p: Point2,
    other: EntityId,
}

/// Which end of a curve.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum End {
    Start,
    End,
}

impl Sketch {
    /// Intersections of `e`'s carrier with other curves (within *their*
    /// extent); `within_self` keeps only those inside `e`'s own extent.
    fn hits(&self, e: EntityId, within_self: bool) -> Result<Vec<Hit>, SketchError> {
        let g = Geom::of(self, e)?;
        let mut out = Vec::new();
        for (other, _) in self.entities() {
            if other == e {
                continue;
            }
            let Ok(h) = Geom::of(self, other) else {
                continue;
            };
            for p in g.carrier_hits(&h) {
                if h.contains(p) && (!within_self || g.contains(p)) {
                    out.push(Hit {
                        t: g.param(p),
                        p,
                        other,
                    });
                }
            }
        }
        Ok(out)
    }

    /// Point-on-curve constraint tying point `p` to curve `e` (Inventor's
    /// Coincident point–curve), for any curve kind. `None` if `p` defines `e`.
    pub fn on_curve(&self, p: PointId, e: EntityId) -> Option<Constraint> {
        match self.entity(e)? {
            x if x.points().contains(&p) => None,
            x if x.is_line() => Some(Constraint::PointOnLine { point: p, line: e }),
            x if x.is_circular() => Some(Constraint::PointOnCircle {
                point: p,
                circle: e,
            }),
            x if x.is_spline() => Some(Constraint::PointOnSpline {
                point: p,
                spline: e,
            }),
            _ => None,
        }
    }

    /// New point at a hit, tied to the boundary curve.
    fn hit_point(&mut self, hit: &Hit) -> Result<PointId, SketchError> {
        let p = self.add_point(hit.p);
        if let Some(c) = self.on_curve(p, hit.other) {
            self.add_constraint(c)?;
        }
        Ok(p)
    }

    /// Replaces the start or end point of a line/arc.
    pub(super) fn set_end(
        &mut self,
        e: EntityId,
        which: End,
        p: PointId,
    ) -> Result<PointId, SketchError> {
        let ent = self.entities.get_mut(e).ok_or(SketchError::UnknownEntity)?;
        let slot = match (ent, which) {
            (Entity::Line { start, .. } | Entity::Arc { start, .. }, End::Start) => start,
            (Entity::Line { end, .. } | Entity::Arc { end, .. }, End::End) => end,
            _ => return Err(SketchError::WrongEntityKind("set_end")),
        };
        Ok(std::mem::replace(slot, p))
    }

    /// Removes `p` if no entity uses it any more (and it is not fixed).
    pub(super) fn drop_orphan(&mut self, p: PointId) -> Result<(), SketchError> {
        let used = self.entities.values().any(|e| e.points().contains(&p));
        if !used && self.point(p).is_some_and(|q| !q.fixed) {
            self.remove_point(p)?;
        }
        Ok(())
    }

    /// Splits curve `e` at hit `h`: `e` keeps `[0, h]`, a new curve takes
    /// `[h, end]` (sharing the split point); the pieces stay on one carrier.
    fn split_at(
        &mut self,
        e: EntityId,
        from: &Hit,
        to: Option<&Hit>,
    ) -> Result<EntityId, SketchError> {
        let a = self.hit_point(from)?;
        let b = match to {
            Some(h) => self.hit_point(h)?,
            None => a,
        };
        let old_end = self.set_end(e, End::End, a)?;
        let piece = match self.entity(e).cloned().ok_or(SketchError::UnknownEntity)? {
            Entity::Line { .. } => {
                let n = self.add_line_between(b, old_end)?;
                self.add_constraint(Constraint::Collinear { a: e, b: n })?;
                n
            }
            Entity::Arc { center, .. } => {
                let n = self.add_arc(center, b, old_end)?;
                self.add_constraint(Constraint::Equal { a: e, b: n })?;
                n
            }
            _ => return Err(SketchError::WrongEntityKind("split")),
        };
        Ok(piece)
    }

    /// **Trim** the part of curve `e` around `at`.
    pub fn trim(&mut self, e: EntityId, at: Point2) -> Result<(), SketchError> {
        let g = Geom::of(self, e)?;
        let t0 = g.param(at);
        let mut hits = self.hits(e, true)?;
        match g {
            Geom::Circle { c, r } => {
                hits.dedup_by(|x, y| (x.p - y.p).norm() < 1e-9);
                if hits.len() < 2 {
                    return self.remove_entity(e).map(|_| ());
                }
                // The circle becomes the arc that does *not* contain `at`.
                let next = |fwd: bool| {
                    hits.iter()
                        .min_by(|x, y| {
                            let d = |h: &Hit| {
                                if fwd {
                                    norm_angle(h.t - t0)
                                } else {
                                    norm_angle(t0 - h.t)
                                }
                            };
                            d(x).total_cmp(&d(y))
                        })
                        .copied()
                };
                // At least two hits (checked above).
                let (Some(hi), Some(lo)) = (next(true), next(false)) else {
                    return Err(SketchError::InvalidValue("trim: no intersections"));
                };
                let Some(&Entity::Circle { center, .. }) = self.entity(e) else {
                    return Err(SketchError::UnknownEntity);
                };
                let _ = (c, r);
                let s = self.hit_point(&hi)?;
                let en = self.hit_point(&lo)?;
                self.entities[e] = Entity::Arc {
                    center,
                    start: s,
                    end: en,
                };
                Ok(())
            }
            _ => {
                let end_t = match g {
                    Geom::Arc { sweep, .. } => sweep,
                    _ => 1.0,
                };
                let inner = |h: &&Hit| h.t > EPS && h.t < end_t - EPS;
                let lo = hits
                    .iter()
                    .filter(inner)
                    .filter(|h| h.t < t0)
                    .max_by(|a, b| a.t.total_cmp(&b.t))
                    .copied();
                let hi = hits
                    .iter()
                    .filter(inner)
                    .filter(|h| h.t > t0)
                    .min_by(|a, b| a.t.total_cmp(&b.t))
                    .copied();
                let ends = self.entity(e).map(Entity::endpoints).unwrap_or_default();
                match (lo, hi) {
                    (None, None) => {
                        self.remove_entity(e)?;
                        for p in ends {
                            self.drop_orphan(p)?;
                        }
                    }
                    (Some(lo), None) => {
                        let p = self.hit_point(&lo)?;
                        let old = self.set_end(e, End::End, p)?;
                        self.drop_orphan(old)?;
                    }
                    (None, Some(hi)) => {
                        let p = self.hit_point(&hi)?;
                        let old = self.set_end(e, End::Start, p)?;
                        self.drop_orphan(old)?;
                    }
                    (Some(lo), Some(hi)) => {
                        self.split_at(e, &lo, Some(&hi))?;
                    }
                }
                Ok(())
            }
        }
    }

    /// **Extend** the end of curve `e` nearest to `at` to the next curve.
    pub fn extend(&mut self, e: EntityId, at: Point2) -> Result<(), SketchError> {
        let g = Geom::of(self, e)?;
        let hits = self.hits(e, false)?;
        let (which, best) = match g {
            Geom::Seg(..) => {
                let which = if g.param(at) < 0.5 {
                    End::Start
                } else {
                    End::End
                };
                let best = match which {
                    End::End => hits
                        .iter()
                        .filter(|h| h.t > 1.0 + EPS)
                        .min_by(|a, b| a.t.total_cmp(&b.t)),
                    End::Start => hits
                        .iter()
                        .filter(|h| h.t < -EPS)
                        .max_by(|a, b| a.t.total_cmp(&b.t)),
                };
                (which, best.copied())
            }
            Geom::Arc { sweep, .. } => {
                let t = g.param(at);
                // Nearer to the end (angle sweep) or to the start (angle 0)?
                let to_end = (t - sweep).abs().min(TAU - t + sweep);
                let to_start = t.min(TAU - t);
                let which = if to_end <= to_start {
                    End::End
                } else {
                    End::Start
                };
                let outside = |h: &&Hit| h.t > sweep + EPS && h.t < TAU - EPS;
                let best = match which {
                    End::End => hits
                        .iter()
                        .filter(outside)
                        .min_by(|a, b| a.t.total_cmp(&b.t)),
                    End::Start => hits
                        .iter()
                        .filter(outside)
                        .max_by(|a, b| a.t.total_cmp(&b.t)),
                };
                (which, best.copied())
            }
            Geom::Circle { .. } => {
                return Err(SketchError::InvalidValue("a circle cannot be extended"));
            }
        };
        let hit = best.ok_or(SketchError::InvalidValue("nothing to extend to"))?;
        let p = self.hit_point(&hit)?;
        let old = self.set_end(e, which, p)?;
        self.drop_orphan(old)
    }

    /// **Split** curve `e` at the intersection nearest to `at`.
    pub fn split(&mut self, e: EntityId, at: Point2) -> Result<EntityId, SketchError> {
        let g = Geom::of(self, e)?;
        if matches!(g, Geom::Circle { .. }) {
            return Err(SketchError::InvalidValue("trim a circle into arcs instead"));
        }
        let end_t = match g {
            Geom::Arc { sweep, .. } => sweep,
            _ => 1.0,
        };
        let t0 = g.param(at);
        let hit = self
            .hits(e, true)?
            .into_iter()
            .filter(|h| h.t > EPS && h.t < end_t - EPS)
            .min_by(|a, b| (a.t - t0).abs().total_cmp(&(b.t - t0).abs()))
            .ok_or(SketchError::InvalidValue("no intersection to split at"))?;
        self.split_at(e, &hit, None)
    }

    /// The chain of lines/arcs connected end to end through `e` (Inventor's
    /// loop select), in traversal order, with `reversed` = traversed
    /// end → start. Also returns whether the chain is closed.
    pub fn chain(&self, e: EntityId) -> (Vec<(EntityId, bool)>, bool) {
        let curve = |x: &Entity| x.is_line() || matches!(x, Entity::Arc { .. });
        // Circles (no endpoints) and other curves form a chain of their own.
        if !self.entity(e).is_some_and(curve) {
            return (vec![(e, false)], false);
        }
        let users = |p: PointId| -> Vec<EntityId> {
            self.entities()
                .filter(|(_, x)| curve(x) && x.endpoints().contains(&p))
                .map(|(id, _)| id)
                .collect()
        };
        let ends = |id: EntityId, rev: bool| {
            let ep = self.entity(id).map(Entity::endpoints).unwrap_or_default();
            if rev { (ep[1], ep[0]) } else { (ep[0], ep[1]) }
        };
        let mut chain = vec![(e, false)];
        // Walk forward from e's end.
        let mut cur = (e, false);
        loop {
            let (_, out) = ends(cur.0, cur.1);
            let next: Vec<EntityId> = users(out).into_iter().filter(|&id| id != cur.0).collect();
            if next.len() != 1 {
                break;
            }
            let n = next[0];
            if n == e {
                return (chain, true);
            }
            if chain.iter().any(|(id, _)| *id == n) {
                break;
            }
            let rev = ends(n, false).0 != out;
            cur = (n, rev);
            chain.push(cur);
        }
        // Walk backward from e's start.
        let mut cur = (e, false);
        loop {
            let (inp, _) = ends(cur.0, cur.1);
            let prev: Vec<EntityId> = users(inp).into_iter().filter(|&id| id != cur.0).collect();
            if prev.len() != 1 || chain.iter().any(|(id, _)| *id == prev[0]) {
                break;
            }
            let p = prev[0];
            let rev = ends(p, false).1 != inp;
            cur = (p, rev);
            chain.insert(0, cur);
        }
        (chain, false)
    }

    /// Signed side of `at` relative to curve `e` in its own direction
    /// (positive = left), for choosing the offset direction.
    pub fn offset_side(&self, e: EntityId, at: Point2) -> Result<Real, SketchError> {
        Ok(match Geom::of(self, e)? {
            Geom::Seg(a, b) => (b - a).normalize().perp(&(at - a)),
            // Left of a counter-clockwise arc is its inside.
            Geom::Arc { c, r, .. } => r - (at - c).norm(),
            Geom::Circle { c, r } => r - (at - c).norm(),
        })
    }

    /// **Offset** curve `e` (or, with `loop_select`, its whole chain) by
    /// `distance` to the left of `e`'s direction. `constrain` adds Inventor's
    /// equal-offset constraints, leaving the distance as the only DOF.
    pub fn offset(
        &mut self,
        e: EntityId,
        distance: Real,
        loop_select: bool,
        constrain: bool,
    ) -> Result<Shape, SketchError> {
        if distance.abs() < 1e-9 {
            return Err(SketchError::InvalidValue("offset distance is zero"));
        }
        let mut sh = Shape::default();
        if let Some(&Entity::Circle { center, radius }) = self.entity(e) {
            let r = radius - distance;
            if r <= 1e-9 {
                return Err(SketchError::InvalidValue(
                    "offset circle would have no radius",
                ));
            }
            let c = self.add_circle_at(center, r)?; // shares the centre: concentric
            sh.entities.push(c);
            return Ok(sh);
        }
        let (chain, closed) = if loop_select {
            self.chain(e)
        } else {
            (vec![(e, false)], false)
        };
        let geoms: Vec<Geom> = chain
            .iter()
            .map(|&(id, _)| Geom::of(self, id))
            .collect::<Result<_, _>>()?;
        // In/out points of each piece in traversal order, offset to the left.
        let mut pieces: Vec<(Point2, Point2)> = Vec::new();
        for (&(_, rev), g) in chain.iter().zip(&geoms) {
            let (p_in, p_out) = match *g {
                Geom::Seg(a, b) => {
                    let (a, b) = if rev { (b, a) } else { (a, b) };
                    let n = (b - a).normalize();
                    let n = Vector2::new(-n.y, n.x) * distance;
                    (a + n, b + n)
                }
                Geom::Arc { c, r, sweep, .. } => {
                    // CCW traversal: left is inwards; CW: outwards.
                    let r2 = if rev { r + distance } else { r - distance };
                    if r2 <= 1e-9 {
                        return Err(SketchError::InvalidValue("offset arc would have no radius"));
                    }
                    let (s, en) = (g.point(0.0), g.point(sweep));
                    let (s, en) = if rev { (en, s) } else { (s, en) };
                    (c + (s - c) * (r2 / r), c + (en - c) * (r2 / r))
                }
                // Circles returned above.
                Geom::Circle { .. } => return Err(SketchError::WrongEntityKind("Offset")),
            };
            pieces.push((p_in, p_out));
        }
        // Corner points: intersection of adjacent offset lines, otherwise the
        // common offset point (tangent joins).
        let n = pieces.len();
        let corner = |i: usize, j: usize| -> Point2 {
            if let (Geom::Seg(..), Geom::Seg(..)) = (geoms[i], geoms[j]) {
                let (a, b) = pieces[i];
                let (c, d) = pieces[j];
                let (r, s) = (b - a, d - c);
                let den = r.perp(&s);
                if den.abs() > 1e-9 * r.norm() * s.norm() {
                    return a + r * ((c - a).perp(&s) / den);
                }
            }
            nalgebra::center(&pieces[i].1, &pieces[j].0)
        };
        let mut verts: Vec<PointId> = Vec::new();
        if closed {
            for i in 0..n {
                verts.push(self.add_point(corner((i + n - 1) % n, i)));
            }
        } else {
            verts.push(self.add_point(pieces[0].0));
            for i in 1..n {
                verts.push(self.add_point(corner(i - 1, i)));
            }
            verts.push(self.add_point(pieces[n - 1].1));
        }
        sh.points.extend(&verts);
        for (i, &(base, rev)) in chain.iter().enumerate() {
            let (p_in, p_out) = (verts[i], verts[(i + 1) % verts.len()]);
            let copy = match self.entity(base).cloned() {
                Some(Entity::Line { .. }) => self.add_line_between(p_in, p_out)?,
                // Shared centre: concentric; CCW orientation preserved.
                Some(Entity::Arc { center, .. }) => {
                    if rev {
                        self.add_arc(center, p_out, p_in)?
                    } else {
                        self.add_arc(center, p_in, p_out)?
                    }
                }
                // The chain's geometry was checked above (lines and arcs).
                _ => return Err(SketchError::WrongEntityKind("Offset")),
            };
            sh.entities.push(copy);
        }
        // Constraints, each added only if it is independent of those before
        // (checked by rank). Smooth joins first: there the corner is where an
        // offset line *touches* an offset circle — a double root that only a
        // first-order tangency (at the shared point) makes solvable.
        let mut candidates = Vec::new();
        let joins = if closed { n } else { n.saturating_sub(1) };
        for i in 0..joins {
            let j = (i + 1) % n;
            if self.tangent_join(chain[i].0, chain[j].0) {
                candidates.push(Constraint::Tangent {
                    a: sh.entities[i],
                    b: sh.entities[j],
                    internal: false,
                });
            }
        }
        for (i, &(base, _)) in chain.iter().enumerate() {
            if self.entity(base).is_some_and(Entity::is_line) {
                candidates.push(Constraint::Parallel {
                    a: base,
                    b: sh.entities[i],
                });
            }
        }
        if constrain {
            for i in 1..n {
                candidates.push(Constraint::EqualOffset {
                    base_a: chain[0].0,
                    copy_a: sh.entities[0],
                    base_b: chain[i].0,
                    copy_b: sh.entities[i],
                });
            }
        }
        for c in candidates {
            if let Some(id) = self.add_if_independent(c)? {
                sh.constraints.push(id);
            }
        }
        Ok(sh)
    }

    /// Do two curves sharing an endpoint meet smoothly (same tangent)?
    fn tangent_join(&self, a: EntityId, b: EntityId) -> bool {
        let (Some(ea), Some(eb)) = (self.entity(a), self.entity(b)) else {
            return false;
        };
        let shared = ea
            .endpoints()
            .into_iter()
            .find(|p| eb.endpoints().contains(p));
        let Some(p) = shared.and_then(|p| self.point(p)).map(|q| q.position) else {
            return false;
        };
        let dir = |e: &Entity| -> Option<Vector2> {
            match e {
                Entity::Line { start, end } => {
                    Some(self.point(*end)?.position - self.point(*start)?.position)
                }
                Entity::Arc { center, .. } => {
                    let r = p - self.point(*center)?.position;
                    Some(Vector2::new(-r.y, r.x))
                }
                _ => None,
            }
        };
        match (dir(ea), dir(eb)) {
            (Some(u), Some(v)) => (u.perp(&v) / (u.norm() * v.norm()).max(1e-12)).abs() < 1e-6,
            _ => false,
        }
    }

    /// Adds `c` unless it is implied by the existing constraints (it would
    /// only add a redundant equation); returns its id when added.
    pub fn add_if_independent(
        &mut self,
        c: Constraint,
    ) -> Result<Option<super::ConstraintId>, SketchError> {
        let before = self.clone().solve()?.redundant;
        let id = self.add_constraint(c)?;
        if self.clone().solve()?.redundant > before {
            self.remove_constraint(id)?;
            return Ok(None);
        }
        Ok(Some(id))
    }

    /// All points of some entities, without duplicates.
    pub fn points_of(&self, entities: &[EntityId]) -> Vec<PointId> {
        let mut out = Vec::new();
        for e in entities {
            for p in self.entity(*e).map(Entity::points).unwrap_or_default() {
                if !out.contains(&p) {
                    out.push(p);
                }
            }
        }
        out
    }

    /// **Move/Rotate**: applies `f` to the free points of `entities`.
    /// Call [`Sketch::solve`] afterwards.
    pub fn transform_geometry(&mut self, entities: &[EntityId], f: impl Fn(Point2) -> Point2) {
        for p in self.points_of(entities) {
            if let Some(q) = self.points.get_mut(p)
                && !q.fixed
            {
                q.position = f(q.position);
            }
        }
    }

    /// Applies `f` to the free points of `entities`, then solves with them
    /// held there so the rest of the sketch follows.
    fn move_and_hold(
        &mut self,
        entities: &[EntityId],
        f: impl Fn(Point2) -> Point2,
    ) -> Result<super::SolveReport, SketchError> {
        let pts: Vec<PointId> = self
            .points_of(entities)
            .into_iter()
            .filter(|&p| self.point(p).is_some_and(|q| !q.fixed))
            .collect();
        self.transform_geometry(entities, f);
        for &p in &pts {
            self.set_fixed(p, true)?;
        }
        let held = self.solve();
        for &p in &pts {
            self.set_fixed(p, false)?;
        }
        // Where the exact target is impossible (e.g. a slightly diagonal
        // pick against H/V constraints), settle at the nearest valid spot,
        // as dragging does.
        let r = match held {
            Ok(r) if r.status == super::SolveStatus::Solved => r,
            _ => self.solve()?,
        };
        if r.status != super::SolveStatus::Solved {
            return Err(SketchError::InvalidValue(
                "the result conflicts with the constraints",
            ));
        }
        Ok(r)
    }

    /// **Scale** `entities` about `base` by `factor` (Inventor: Sketch →
    /// Modify → Scale). Circle radii and the dimensions measured entirely
    /// within the selection scale with it; the rest of the sketch follows.
    pub fn scale_geometry(
        &mut self,
        entities: &[EntityId],
        base: Point2,
        factor: Real,
    ) -> Result<(), SketchError> {
        if !(factor.is_finite() && factor > 0.0) {
            return Err(SketchError::InvalidValue(
                "the scale factor must be positive",
            ));
        }
        let pts = self.points_of(entities);
        for &e in entities {
            if let Some(Entity::Circle { radius, .. }) = self.entities.get_mut(e) {
                *radius *= factor;
            }
        }
        let inside = |c: &Constraint| match *c {
            Constraint::Distance { a, b, .. }
            | Constraint::HorizontalDistance { a, b, .. }
            | Constraint::VerticalDistance { a, b, .. } => pts.contains(&a) && pts.contains(&b),
            Constraint::PointLineDistance { point, line, .. }
            | Constraint::DiameterToAxis { point, line, .. } => {
                pts.contains(&point) && entities.contains(&line)
            }
            Constraint::Radius { circle, .. } | Constraint::Diameter { circle, .. } => {
                entities.contains(&circle)
            }
            _ => false,
        };
        for c in self.constraints.values_mut() {
            if inside(c)
                && let Some(v) = c.value_mut()
            {
                *v *= factor;
            }
        }
        self.move_and_hold(entities, |q| base + (q - base) * factor)
            .map(|_| ())
    }

    /// **Stretch**: moves the points of `entities` by `delta`; connected
    /// geometry stretches and the dimensions take their new values
    /// (Inventor: Sketch → Modify → Stretch).
    pub fn stretch_geometry(
        &mut self,
        entities: &[EntityId],
        delta: Vector2,
    ) -> Result<(), SketchError> {
        // Only dimensions spanning moved and unmoved geometry give way; those
        // within the selection (or outside it) keep their values.
        let moved = self.points_of(entities);
        let refs = |c: &Constraint| -> Vec<PointId> {
            let mut out: Vec<PointId> = self
                .points()
                .map(|(p, _)| p)
                .filter(|&p| c.references_point(p))
                .collect();
            for (e, ent) in self.entities() {
                if c.references_entity(e) {
                    out.extend(ent.points());
                }
            }
            out
        };
        let crossing: Vec<Constraint> = self
            .constraints()
            .map(|(_, c)| c)
            .filter(|c| {
                let r = refs(c);
                r.iter().any(|p| moved.contains(p)) && r.iter().any(|p| !moved.contains(p))
            })
            .cloned()
            .collect();
        let relaxed = self.relax_dimensions_where(|c| crossing.contains(c));
        let r = self.move_and_hold(entities, |q| q + delta);
        self.remeasure_dimensions(&relaxed);
        r.map(|_| ())
    }

    /// **Copy**: duplicates `entities` through `f` (with their internal
    /// constraints, except Fix). With `mirror`, arcs are re-oriented and
    /// points that `f` leaves in place are shared instead of duplicated.
    pub fn copy_geometry(
        &mut self,
        entities: &[EntityId],
        f: impl Fn(Point2) -> Point2,
        mirror: bool,
        copy_constraints: bool,
    ) -> Result<(Shape, HashMap<PointId, PointId>), SketchError> {
        self.copy_geometry_keeping(entities, f, mirror, copy_constraints, None)
    }

    /// [`Sketch::copy_geometry`] that leaves point `keep` shared (the
    /// centre of a circular pattern: a radial line's copies all start at it).
    pub(crate) fn copy_geometry_keeping(
        &mut self,
        entities: &[EntityId],
        f: impl Fn(Point2) -> Point2,
        mirror: bool,
        copy_constraints: bool,
        keep: Option<PointId>,
    ) -> Result<(Shape, HashMap<PointId, PointId>), SketchError> {
        let mut sh = Shape::default();
        let mut map: HashMap<PointId, PointId> = HashMap::new();
        for p in self.points_of(entities) {
            let old = self.point(p).ok_or(SketchError::UnknownPoint)?.position;
            let new = f(old);
            let id = if (mirror && (new - old).norm() < 1e-9) || keep == Some(p) {
                p
            } else {
                let id = self.add_point(new);
                sh.points.push(id);
                id
            };
            map.insert(p, id);
        }
        let mut emap: HashMap<EntityId, EntityId> = HashMap::new();
        for &e in entities {
            let style = self.style(e);
            let id = match self.entity(e).cloned().ok_or(SketchError::UnknownEntity)? {
                Entity::Line { start, end } => self.add_line_between(map[&start], map[&end])?,
                Entity::Circle { center, radius } => self.add_circle_at(map[&center], radius)?,
                Entity::Arc { center, start, end } => {
                    if mirror {
                        self.add_arc(map[&center], map[&end], map[&start])?
                    } else {
                        self.add_arc(map[&center], map[&start], map[&end])?
                    }
                }
                Entity::Spline { points } => {
                    let id = self.add_spline(points.iter().map(|p| map[p]).collect())?;
                    // Interpolation splines stay so; handles are vectors,
                    // carried by the same transform.
                    if let Some(fit) = self.interp.get(e).cloned() {
                        let p0 = self
                            .point(points[0])
                            .map_or(Point2::origin(), |q| q.position);
                        let pn = self
                            .point(points[points.len() - 1])
                            .map_or(Point2::origin(), |q| q.position);
                        let map_v = |at: Point2, v: Option<[Real; 2]>| {
                            v.map(|v| {
                                let d = f(at + Vector2::new(v[0], v[1])) - f(at);
                                [d.x, d.y]
                            })
                        };
                        // The point order stays, so the mapped vectors are
                        // right for mirrors too.
                        let (start, end) = (map_v(p0, fit.start), map_v(pn, fit.end));
                        let inner = fit
                            .inner
                            .iter()
                            .enumerate()
                            .map(|(k, v)| {
                                let at = points
                                    .get(k)
                                    .and_then(|&p| self.point(p))
                                    .map_or(Point2::origin(), |q| q.position);
                                map_v(at, *v)
                            })
                            .collect();
                        self.interp.insert(
                            id,
                            super::SplineFit {
                                start,
                                end,
                                inner,
                                closed: fit.closed,
                            },
                        );
                    }
                    id
                }
            };
            self.set_style(id, style)?;
            emap.insert(e, id);
            sh.entities.push(id);
        }
        if copy_constraints {
            let inside: Vec<Constraint> = self
                .constraints()
                .map(|(_, c)| c.clone())
                .filter(|c| !matches!(c, Constraint::Fix { .. }))
                .filter_map(|c| remap(&c, &map, &emap))
                .collect();
            for c in inside {
                sh.constraints.push(self.add_constraint(c)?);
            }
        }
        Ok((sh, map))
    }

    /// **Mirror** `entities` about `line`: a mirrored copy tied by Symmetric
    /// constraints (plus Equal for circles, whose radius is not a point).
    pub fn mirror_geometry(
        &mut self,
        entities: &[EntityId],
        line: EntityId,
    ) -> Result<Shape, SketchError> {
        let (l0, l1) = self
            .line_geometry(line)
            .ok_or(SketchError::WrongEntityKind("Mirror"))?;
        let d = (l1 - l0).normalize();
        let reflect = move |q: Point2| {
            let v = q - l0;
            l0 + d * (2.0 * v.dot(&d)) - v
        };
        let entities: Vec<EntityId> = entities.iter().copied().filter(|&e| e != line).collect();
        let (mut sh, map) = self.copy_geometry(&entities, reflect, true, false)?;
        // A mirrored arc already has its "end on the circle" equation: tie one
        // of its non-shared points by one equation only (see SymmetricOnArc).
        let mut reduced: HashMap<PointId, PointId> = HashMap::new();
        for e in &entities {
            if let Some(&Entity::Arc { center, start, end }) = self.entity(*e) {
                let free = |p: &PointId| map[p] != *p && !reduced.contains_key(p);
                let pick = [(end, center), (start, center), (center, end)]
                    .into_iter()
                    .find(|(p, _)| free(p));
                if let Some((p, about)) = pick {
                    reduced.insert(p, map[&about]);
                }
            }
        }
        for (&a, &b) in &map {
            if a != b {
                let c = match reduced.get(&a) {
                    Some(&about) => Constraint::SymmetricOnArc { a, b, line, about },
                    None => Constraint::Symmetric { a, b, line },
                };
                sh.constraints.push(self.add_constraint(c)?);
            }
        }
        for (i, &e) in entities.iter().enumerate() {
            if matches!(self.entity(e), Some(Entity::Circle { .. })) {
                sh.constraints.push(self.add_constraint(Constraint::Equal {
                    a: e,
                    b: sh.entities[i],
                })?);
            }
        }
        Ok(sh)
    }
}

/// A constraint whose every reference lies inside the copied set,
/// rewritten to the copies.
fn remap(
    c: &Constraint,
    pm: &HashMap<PointId, PointId>,
    em: &HashMap<EntityId, EntityId>,
) -> Option<Constraint> {
    let p = |x: PointId| pm.get(&x).copied();
    let e = |x: EntityId| em.get(&x).copied();
    Some(match *c {
        Constraint::Coincident { a, b } => Constraint::Coincident { a: p(a)?, b: p(b)? },
        Constraint::PointOnLine { point, line } => Constraint::PointOnLine {
            point: p(point)?,
            line: e(line)?,
        },
        Constraint::PointOnCircle { point, circle } => Constraint::PointOnCircle {
            point: p(point)?,
            circle: e(circle)?,
        },
        Constraint::PointOnSpline { point, spline } => Constraint::PointOnSpline {
            point: p(point)?,
            spline: e(spline)?,
        },
        Constraint::Translated {
            a,
            a2,
            b,
            b2,
            arc_center,
            offset,
        } => Constraint::Translated {
            a: p(a)?,
            a2: p(a2)?,
            b: p(b)?,
            b2: p(b2)?,
            arc_center: match arc_center {
                Some(c) => Some(p(c)?),
                None => None,
            },
            offset,
        },
        Constraint::Rotated {
            center,
            a,
            a2,
            b,
            b2,
            arc_center,
        } => Constraint::Rotated {
            center: p(center)?,
            a: p(a)?,
            a2: p(a2)?,
            b: p(b)?,
            b2: p(b2)?,
            arc_center: match arc_center {
                Some(c) => Some(p(c)?),
                None => None,
            },
        },
        Constraint::Midpoint { point, line } => Constraint::Midpoint {
            point: p(point)?,
            line: e(line)?,
        },
        Constraint::Collinear { a, b } => Constraint::Collinear { a: e(a)?, b: e(b)? },
        Constraint::Concentric { a, b } => Constraint::Concentric { a: e(a)?, b: e(b)? },
        Constraint::Horizontal { line } => Constraint::Horizontal { line: e(line)? },
        Constraint::Vertical { line } => Constraint::Vertical { line: e(line)? },
        Constraint::HorizontalPoints { a, b } => {
            Constraint::HorizontalPoints { a: p(a)?, b: p(b)? }
        }
        Constraint::VerticalPoints { a, b } => Constraint::VerticalPoints { a: p(a)?, b: p(b)? },
        Constraint::Parallel { a, b } => Constraint::Parallel { a: e(a)?, b: e(b)? },
        Constraint::Perpendicular { a, b } => Constraint::Perpendicular { a: e(a)?, b: e(b)? },
        Constraint::Tangent { a, b, internal } => Constraint::Tangent {
            a: e(a)?,
            b: e(b)?,
            internal,
        },
        Constraint::Equal { a, b } => Constraint::Equal { a: e(a)?, b: e(b)? },
        Constraint::SymmetricOnArc { a, b, line, about } => Constraint::SymmetricOnArc {
            a: p(a)?,
            b: p(b)?,
            line: e(line)?,
            about: p(about)?,
        },
        Constraint::Symmetric { a, b, line } => Constraint::Symmetric {
            a: p(a)?,
            b: p(b)?,
            line: e(line)?,
        },
        Constraint::EqualOffset {
            base_a,
            copy_a,
            base_b,
            copy_b,
        } => Constraint::EqualOffset {
            base_a: e(base_a)?,
            copy_a: e(copy_a)?,
            base_b: e(base_b)?,
            copy_b: e(copy_b)?,
        },
        Constraint::Distance { a, b, value } => Constraint::Distance {
            a: p(a)?,
            b: p(b)?,
            value,
        },
        Constraint::HorizontalDistance { a, b, value } => Constraint::HorizontalDistance {
            a: p(a)?,
            b: p(b)?,
            value,
        },
        Constraint::VerticalDistance { a, b, value } => Constraint::VerticalDistance {
            a: p(a)?,
            b: p(b)?,
            value,
        },
        Constraint::PointLineDistance { point, line, value } => Constraint::PointLineDistance {
            point: p(point)?,
            line: e(line)?,
            value,
        },
        Constraint::DiameterToAxis { point, line, value } => Constraint::DiameterToAxis {
            point: p(point)?,
            line: e(line)?,
            value,
        },
        Constraint::Angle {
            a,
            b,
            value,
            supplement,
        } => Constraint::Angle {
            a: e(a)?,
            b: e(b)?,
            value,
            supplement,
        },
        Constraint::Radius { circle, value } => Constraint::Radius {
            circle: e(circle)?,
            value,
        },
        Constraint::Diameter { circle, value } => Constraint::Diameter {
            circle: e(circle)?,
            value,
        },
        Constraint::Fix { .. } => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sketch::SolveStatus;
    use approx::assert_relative_eq;

    fn p(x: Real, y: Real) -> Point2 {
        Point2::new(x, y)
    }

    fn solved(s: &mut Sketch) -> crate::sketch::SolveReport {
        let r = s.solve().unwrap();
        assert_eq!(r.status, SolveStatus::Solved, "{r:?}");
        r
    }

    /// A horizontal line crossed by two vertical ones at x = 10 and x = 20.
    fn cross() -> (Sketch, EntityId) {
        let mut s = Sketch::default();
        let h = s.add_line(p(0.0, 0.0), p(30.0, 0.0));
        s.add_line(p(10.0, -5.0), p(10.0, 5.0));
        s.add_line(p(20.0, -5.0), p(20.0, 5.0));
        (s, h)
    }

    #[test]
    fn trim_middle_splits_into_collinear_pieces() {
        let (mut s, h) = cross();
        s.trim(h, p(15.0, 0.0)).unwrap();
        solved(&mut s);
        let (a, b) = s.line_geometry(h).unwrap();
        assert_relative_eq!(a.x, 0.0, epsilon = 1e-9);
        assert_relative_eq!(b.x, 10.0, epsilon = 1e-9);
        assert_eq!(s.entities().count(), 4);
        assert!(
            s.constraints()
                .any(|(_, c)| matches!(c, Constraint::Collinear { .. }))
        );
        assert_eq!(
            s.constraints()
                .filter(|(_, c)| matches!(c, Constraint::PointOnLine { .. }))
                .count(),
            2
        );
    }

    #[test]
    fn trim_end_shortens_and_trim_free_curve_deletes() {
        let (mut s, h) = cross();
        s.trim(h, p(25.0, 0.0)).unwrap();
        let (_, b) = s.line_geometry(h).unwrap();
        assert_relative_eq!(b.x, 20.0, epsilon = 1e-9);
        let lonely = s.add_line(p(0.0, 50.0), p(5.0, 50.0));
        s.trim(lonely, p(2.0, 50.0)).unwrap();
        assert!(s.entity(lonely).is_none());
        solved(&mut s);
    }

    #[test]
    fn trim_circle_leaves_the_far_arc() {
        let mut s = Sketch::default();
        let c = s.add_circle(p(0.0, 0.0), 10.0).unwrap();
        s.add_line(p(-20.0, 0.0), p(20.0, 0.0));
        s.trim(c, p(0.0, 10.0)).unwrap(); // remove the top half
        assert!(matches!(s.entity(c), Some(Entity::Arc { .. })));
        solved(&mut s);
        // The remaining arc passes through the bottom.
        let g = Geom::of(&s, c).unwrap();
        assert!(g.contains(p(0.0, -10.0)) && !g.contains(p(0.0, 10.0)));
    }

    #[test]
    fn extend_to_nearest_boundary() {
        let (mut s, _) = cross();
        let short = s.add_line(p(0.0, 10.0), p(5.0, 10.0));
        let wall = s.add_line(p(12.0, 0.0), p(12.0, 20.0));
        s.extend(short, p(4.0, 10.0)).unwrap();
        solved(&mut s);
        let (_, b) = s.line_geometry(short).unwrap();
        assert_relative_eq!(b.x, 12.0, epsilon = 1e-9);
        assert!(
            s.constraints()
                .any(|(_, c)| matches!(c, Constraint::PointOnLine { line, .. } if *line == wall))
        );
        // A line going nowhere cannot be extended.
        let far = s.add_line(p(100.0, 100.0), p(101.0, 100.0));
        assert!(s.extend(far, p(101.0, 100.0)).is_err());
    }

    #[test]
    fn split_keeps_both_pieces() {
        let (mut s, h) = cross();
        let piece = s.split(h, p(12.0, 0.0)).unwrap();
        solved(&mut s);
        let (_, b) = s.line_geometry(h).unwrap();
        let (c, d) = s.line_geometry(piece).unwrap();
        assert_relative_eq!(b.x, 10.0, epsilon = 1e-9);
        assert_relative_eq!(c.x, 10.0, epsilon = 1e-9);
        assert_relative_eq!(d.x, 30.0, epsilon = 1e-9);
    }

    #[test]
    fn offset_loop_has_one_dof_left() {
        // Fully constrained rectangle, offset inwards.
        let mut s = Sketch::default();
        let sh = s
            .add_rectangle_two_point(p(0.0, 0.0), p(40.0, 20.0))
            .unwrap();
        s.add_constraint(Constraint::Fix {
            point: sh.points[0],
            at: p(0.0, 0.0),
        })
        .unwrap();
        s.add_constraint(Constraint::Distance {
            a: sh.points[0],
            b: sh.points[1],
            value: 40.0,
        })
        .unwrap();
        s.add_constraint(Constraint::Distance {
            a: sh.points[1],
            b: sh.points[2],
            value: 20.0,
        })
        .unwrap();
        assert_eq!(solved(&mut s).dof, 0);
        let bottom = sh.entities[0]; // runs +X: left is inside
        let off = s.offset(bottom, 3.0, true, true).unwrap();
        assert_eq!(off.entities.len(), 4);
        let r = solved(&mut s);
        assert_eq!((r.dof, r.redundant), (1, 0), "{r:?}");
        let (a, b) = s.line_geometry(off.entities[0]).unwrap();
        assert_relative_eq!(a, p(3.0, 3.0), epsilon = 1e-9);
        assert_relative_eq!(b, p(37.0, 3.0), epsilon = 1e-9);
    }

    #[test]
    fn chain_of_a_circle_is_the_circle() {
        // Regression: the Offset preview chained a circle inside a line loop
        // and indexed its (missing) endpoints — the whole app panicked.
        let mut s = Sketch::default();
        s.add_rectangle_two_point(p(0.0, 0.0), p(40.0, 20.0))
            .unwrap();
        let c = s.add_circle(p(20.0, 10.0), 5.0).unwrap();
        assert_eq!(s.chain(c), (vec![(c, false)], false));
        let off = s.offset(c, -2.0, true, true).unwrap();
        assert_relative_eq!(
            s.circle_geometry(off.entities[0]).unwrap().1,
            7.0,
            epsilon = 1e-9
        );
    }

    #[test]
    fn offset_slot_with_arcs() {
        let mut s = Sketch::default();
        let sh = s
            .add_slot_center_to_center(p(0.0, 0.0), p(30.0, 0.0), 10.0)
            .unwrap();
        let top = sh.entities[2]; // b_top → a_top, runs −X: left is outside
        let off = s.offset(top, 2.0, true, true).unwrap();
        assert_eq!(off.entities.len(), 4, "two lines and two arcs");
        let r = solved(&mut s);
        assert_eq!(r.redundant, 0, "{r:?}");
        // The top line runs −X, so its left is the inside: inner slot, arcs r = 3.
        let arc = off
            .entities
            .iter()
            .find(|&&e| matches!(s.entity(e), Some(Entity::Arc { .. })))
            .unwrap();
        assert_relative_eq!(s.circle_geometry(*arc).unwrap().1, 3.0, epsilon = 1e-9);
        assert_eq!(r.dof, 5 + 1, "slot DOF + the offset distance");
    }

    #[test]
    fn mirrored_arcs_are_not_over_constrained() {
        // Regression: Symmetric on all three points of a mirrored arc plus
        // the arc's own end-on-circle equation gave 1 redundant equation.
        // Ends on the axis: centre off it (centre reduced), and on it (all
        // points shared: two halves of one circle).
        for (case, (c, a, b)) in [
            (p(10.0, 0.0), p(13.0, 0.0), p(10.0, 3.0)),
            (p(3.0, 0.0), p(0.0, -4.0), p(0.0, 4.0)),
            (p(0.0, 0.0), p(0.0, -5.0), p(0.0, 5.0)),
        ]
        .into_iter()
        .enumerate()
        {
            let mut s = Sketch::default();
            let a0 = s.add_fixed_point(p(0.0, -50.0));
            let a1 = s.add_fixed_point(p(0.0, 50.0));
            let axis = s.add_line_between(a0, a1).unwrap();
            let (c, a, b) = (s.add_point(c), s.add_point(a), s.add_point(b));
            let arc = s.add_arc(c, a, b).unwrap();
            let dof = solved(&mut s).dof;
            s.mirror_geometry(&[arc], axis).unwrap();
            let r = solved(&mut s);
            assert_eq!((r.dof, r.redundant), (dof, 0), "case {case}: {r:?}");
        }
    }

    #[test]
    fn scale_and_stretch_update_dimensions() {
        let mut s = Sketch::default();
        let sh = s
            .add_rectangle_two_point(p(0.0, 0.0), p(40.0, 20.0))
            .unwrap();
        s.set_fixed(sh.points[0], true).unwrap();
        let w = s
            .add_constraint(Constraint::Distance {
                a: sh.points[0],
                b: sh.points[1],
                value: 40.0,
            })
            .unwrap();
        let h = s
            .add_constraint(Constraint::Distance {
                a: sh.points[1],
                b: sh.points[2],
                value: 20.0,
            })
            .unwrap();
        let hole = s.add_circle(p(10.0, 10.0), 3.0).unwrap();
        let mut all = sh.entities.clone();
        all.push(hole);
        s.scale_geometry(&all, p(0.0, 0.0), 2.0).unwrap();
        assert_eq!(s.constraint(w).and_then(Constraint::value), Some(80.0));
        assert_eq!(s.constraint(h).and_then(Constraint::value), Some(40.0));
        assert_relative_eq!(s.circle_geometry(hole).unwrap().1, 6.0, epsilon = 1e-12);
        assert_eq!(solved(&mut s).redundant, 0);
        // Stretch the right side 10 mm further — with a slightly diagonal
        // pick, as in the GUI: the width becomes 90, the rectangle stays H/V.
        let right = sh.entities[1];
        s.stretch_geometry(&[right], Vector2::new(10.0, 0.3))
            .unwrap();
        assert_relative_eq!(s.measure(w).unwrap(), 90.0, epsilon = 1e-9);
        assert_eq!(
            s.constraint(h).and_then(Constraint::value),
            Some(40.0),
            "inside: kept"
        );
        assert_relative_eq!(
            s.constraint(w).and_then(Constraint::value).unwrap(),
            90.0,
            epsilon = 1e-9
        );
        assert!(!s.is_driven(w));
        assert!(s.scale_geometry(&all, p(0.0, 0.0), 0.0).is_err());
    }

    #[test]
    fn mirror_and_copy() {
        let mut s = Sketch::default();
        let a0 = s.add_fixed_point(p(0.0, -50.0));
        let a1 = s.add_fixed_point(p(0.0, 50.0));
        let axis = s.add_line_between(a0, a1).unwrap();
        let l = s.add_line(p(5.0, 0.0), p(15.0, 5.0));
        s.add_constraint(Constraint::Horizontal { line: l })
            .unwrap();
        let c = s.add_circle(p(10.0, 20.0), 3.0).unwrap();
        let m = s.mirror_geometry(&[l, c], axis).unwrap();
        let r = solved(&mut s);
        assert_eq!(r.redundant, 0, "{r:?}");
        let (q0, _) = s.line_geometry(m.entities[0]).unwrap();
        let (p0, _) = s.line_geometry(l).unwrap();
        assert_relative_eq!(q0, p(-p0.x, p0.y), epsilon = 1e-8);
        assert_relative_eq!(
            s.circle_geometry(m.entities[1]).unwrap().1,
            s.circle_geometry(c).unwrap().1,
            epsilon = 1e-9
        );

        let (copy, _) = s
            .copy_geometry(&[l], |q| q + Vector2::new(0.0, 30.0), false, true)
            .unwrap();
        assert!(
            s.constraints()
                .filter(|(_, k)| matches!(k, Constraint::Horizontal { .. }))
                .count()
                == 2
        );
        assert_eq!(copy.entities.len(), 1);
        solved(&mut s);
    }
}
