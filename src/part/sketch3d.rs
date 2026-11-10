//! **3D sketch** (Inventor: 3D Sketch, `docs/spec/1.10-3d-sketch.md`): points
//! and straight lines in part space, tangent bends of a radius at the
//! corners of two lines, Parallel / Perpendicular constraints between lines
//! and fixed points. A chain of lines and bends is a path for Sweep
//! ([`Sketch3d::pieces`]).
//!
//! The geometry is stored in part coordinates (rule 8); ids of points and
//! lines are the sketch's own and never reused.

use serde::{Deserialize, Serialize};

use crate::kernel::exact::PathPiece;
use crate::kernel::{Point3, Real, Vector3};

/// A point of a 3D sketch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Point3d {
    /// Its id in the sketch.
    pub id: u32,
    /// Where it is (part coordinates, mm).
    pub at: [Real; 3],
    /// It keeps its place when the sketch is solved.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fixed: bool,
}

/// A straight line between two points.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line3d {
    /// Its id in the sketch.
    pub id: u32,
    /// Start point id.
    pub a: u32,
    /// End point id.
    pub b: u32,
}

/// A tangent bend at the corner point of two lines.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bend3d {
    /// The point the two lines share.
    pub corner: u32,
    /// The bend's radius (mm, > 0).
    pub radius: Real,
}

/// A constraint between two lines of the sketch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Constraint3 {
    /// The two lines run the same way (or opposite).
    Parallel(u32, u32),
    /// The two lines are square to each other.
    Perpendicular(u32, u32),
}

/// A 3D sketch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sketch3d {
    /// Immutable identity (features refer to it).
    pub id: String,
    /// Display name.
    pub name: String,
    /// Its points.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub points: Vec<Point3d>,
    /// Its lines.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<Line3d>,
    /// Bends at corners.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bends: Vec<Bend3d>,
    /// Constraints.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<Constraint3>,
    /// Shown in the view.
    #[serde(default = "yes")]
    pub visible: bool,
    /// The next id for a point or line.
    #[serde(default)]
    pub next_id: u32,
}

fn yes() -> bool {
    true
}

/// Points along path pieces (arcs within 0.01 mm).
pub fn points_of(p: &[PathPiece]) -> Vec<Point3> {
    crate::kernel::exact::path_points(p)
        .into_iter()
        .map(|q| Point3::new(q.x, q.y, q.z))
        .collect()
}

fn v(a: [Real; 3]) -> Vector3 {
    Vector3::new(a[0], a[1], a[2])
}

impl Sketch3d {
    /// An empty 3D sketch.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            points: Vec::new(),
            lines: Vec::new(),
            bends: Vec::new(),
            constraints: Vec::new(),
            visible: true,
            next_id: 1,
        }
    }

    fn take_id(&mut self) -> u32 {
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        id
    }

    /// The point with id `id`.
    pub fn point(&self, id: u32) -> Option<&Point3d> {
        self.points.iter().find(|p| p.id == id)
    }

    /// Where point `id` is.
    pub fn at(&self, id: u32) -> Option<Point3> {
        self.point(id).map(|p| Point3::from(p.at))
    }

    /// The line with id `id`.
    pub fn line(&self, id: u32) -> Option<&Line3d> {
        self.lines.iter().find(|l| l.id == id)
    }

    /// Adds a point; returns its id. A point within 1e-9 mm of an existing
    /// one is that point.
    pub fn add_point(&mut self, at: Point3, fixed: bool) -> u32 {
        if let Some(p) = self
            .points
            .iter_mut()
            .find(|p| (Point3::from(p.at) - at).norm() < 1e-9)
        {
            p.fixed |= fixed;
            return p.id;
        }
        let id = self.take_id();
        self.points.push(Point3d {
            id,
            at: [at.x, at.y, at.z],
            fixed,
        });
        id
    }

    /// Adds a line between two points of the sketch; returns its id.
    pub fn add_line(&mut self, a: u32, b: u32) -> Result<u32, String> {
        let (Some(pa), Some(pb)) = (self.at(a), self.at(b)) else {
            return Err("the line's points are not in the sketch".into());
        };
        if a == b || (pa - pb).norm() < 1e-9 {
            return Err("the line has no length".into());
        }
        let id = self.take_id();
        self.lines.push(Line3d { id, a, b });
        Ok(id)
    }

    /// The lines ending at point `p`.
    pub fn lines_at(&self, p: u32) -> Vec<&Line3d> {
        self.lines.iter().filter(|l| l.a == p || l.b == p).collect()
    }

    /// **Coincident**: point `drop` becomes point `keep` (its lines,
    /// bends and place). Lines that would lose their length are refused.
    pub fn merge_points(&mut self, keep: u32, drop: u32) -> Result<(), String> {
        if keep == drop {
            return Ok(());
        }
        if self.point(keep).is_none() || self.point(drop).is_none() {
            return Err("the points are not in the sketch".into());
        }
        if self
            .lines
            .iter()
            .any(|l| (l.a == keep && l.b == drop) || (l.a == drop && l.b == keep))
        {
            return Err("the two points are the ends of one line".into());
        }
        let mut next = self.clone();
        for l in &mut next.lines {
            if l.a == drop {
                l.a = keep;
            }
            if l.b == drop {
                l.b = keep;
            }
        }
        next.bends.retain(|b| b.corner != drop);
        let fixed = next.point(drop).is_some_and(|p| p.fixed);
        next.points.retain(|p| p.id != drop);
        if let Some(p) = next.points.iter_mut().find(|p| p.id == keep) {
            p.fixed |= fixed;
        }
        next.solve()?;
        *self = next;
        Ok(())
    }

    /// Adds a constraint and solves; a constraint that cannot be met leaves
    /// the sketch unchanged (rule 5).
    pub fn add_constraint(&mut self, c: Constraint3) -> Result<(), String> {
        let (Constraint3::Parallel(a, b) | Constraint3::Perpendicular(a, b)) = c;
        if a == b || self.line(a).is_none() || self.line(b).is_none() {
            return Err("pick two lines of the sketch".into());
        }
        if self.constraints.contains(&c) {
            return Ok(());
        }
        let mut next = self.clone();
        next.constraints.push(c);
        next.solve()?;
        // Bends must still fit.
        if !next.bends.is_empty() {
            next.pieces()?;
        }
        *self = next;
        Ok(())
    }

    /// Adds (or changes) a bend of `radius` at the corner point `corner`;
    /// refused when the corner is not two lines meeting at an angle or
    /// the radius does not fit between it and the lines' other ends.
    pub fn add_bend(&mut self, corner: u32, radius: Real) -> Result<(), String> {
        if !(radius.is_finite() && radius > 0.0) {
            return Err("Bend: the radius must be above 0".into());
        }
        if self.lines_at(corner).len() != 2 {
            return Err("Bend: pick a point where two lines meet".into());
        }
        let mut next = self.clone();
        next.bends.retain(|b| b.corner != corner);
        next.bends.push(Bend3d { corner, radius });
        next.pieces()?;
        *self = next;
        Ok(())
    }

    /// The unit direction of line `l` from its point `from`.
    fn dir_from(&self, l: &Line3d, from: u32) -> Option<(Vector3, Real)> {
        let (a, b) = (self.at(l.a)?, self.at(l.b)?);
        let d = if from == l.a { b - a } else { a - b };
        let n = d.norm();
        (n > 1e-12).then(|| (d / n, n))
    }

    /// The bend at `corner`: (trim length along each line, the arc) or
    /// `None` when there is none.
    fn bend_at(&self, corner: u32) -> Result<Option<(Real, PathPiece, Point3, Point3)>, String> {
        let Some(b) = self.bends.iter().find(|b| b.corner == corner) else {
            return Ok(None);
        };
        let ls = self.lines_at(corner);
        let [l1, l2] = ls.as_slice() else {
            return Err("Bend: the corner is no longer two lines".into());
        };
        let c = self.at(corner).ok_or("Bend: the corner point is gone")?;
        // u1 runs into the corner, u2 out of it.
        let (back, _) = self
            .dir_from(l1, corner)
            .ok_or("Bend: a line has no length")?;
        let (u2, _) = self
            .dir_from(l2, corner)
            .ok_or("Bend: a line has no length")?;
        let u1 = -back;
        let cos = u1.dot(&u2).clamp(-1.0, 1.0);
        let phi = cos.acos();
        if phi < 1e-9 {
            return Ok(None); // straight on: nothing to bend
        }
        if (std::f64::consts::PI - phi) < 1e-9 {
            return Err("Bend: the lines fold back on each other".into());
        }
        let t = b.radius * (phi / 2.0).tan();
        let n = (u2 - u1 * cos)
            .try_normalize(1e-12)
            .ok_or("Bend: the lines are parallel")?;
        let t1 = c - u1 * t;
        let t2 = c + u2 * t;
        let center = t1 + n * b.radius;
        let axis = u1.cross(&n);
        Ok(Some((
            t,
            PathPiece::Arc {
                a: t1,
                center,
                axis,
                angle: phi,
            },
            t1,
            t2,
        )))
    }

    /// The chains of lines (point ids in order), each from an end (or
    /// round a closed loop). A point where three lines meet is refused.
    pub fn chains(&self) -> Result<Vec<Vec<u32>>, String> {
        for p in &self.points {
            if self.lines_at(p.id).len() > 2 {
                return Err("the path branches at a point".into());
            }
        }
        let mut used: Vec<u32> = Vec::new();
        let mut out = Vec::new();
        // Open chains start at a point with one line; closed ones anywhere.
        let mut starts: Vec<u32> = self
            .points
            .iter()
            .filter(|p| self.lines_at(p.id).len() == 1)
            .map(|p| p.id)
            .collect();
        starts.extend(
            self.points
                .iter()
                .filter(|p| self.lines_at(p.id).len() == 2)
                .map(|p| p.id),
        );
        for s in starts {
            let Some(first) = self.lines_at(s).into_iter().find(|l| !used.contains(&l.id)) else {
                continue;
            };
            let mut chain = vec![s];
            let mut line = first.clone();
            let mut at = s;
            loop {
                used.push(line.id);
                let next = if line.a == at { line.b } else { line.a };
                chain.push(next);
                at = next;
                match self
                    .lines_at(at)
                    .into_iter()
                    .find(|l| !used.contains(&l.id))
                {
                    Some(l) => line = l.clone(),
                    None => break,
                }
            }
            out.push(chain);
        }
        Ok(out)
    }

    /// The sketch as one path: its lines in chain order, shortened by the
    /// bends, and the bends' arcs. Refused when the lines make more than
    /// one chain or branch, or a bend does not fit.
    pub fn pieces(&self) -> Result<Vec<PathPiece>, String> {
        let chains = self.chains()?;
        let Some(chain) = chains.first() else {
            return Err("the 3D sketch has no lines".into());
        };
        if chains.len() > 1 {
            return Err("the 3D sketch has more than one chain of lines".into());
        }
        let closed = chain.len() > 2 && chain.first() == chain.last();
        let n = chain.len() - 1;
        // Bends at the chain's inner corners (and the start of a loop).
        let mut bends = Vec::with_capacity(chain.len());
        for (k, &p) in chain.iter().enumerate() {
            let inner = (k > 0 && k < n) || (closed && k == 0);
            bends.push(if inner { self.bend_at(p)? } else { None });
        }
        if closed {
            bends[n] = bends[0].clone();
        }
        let mut out = Vec::new();
        for k in 0..n {
            let (a, b) = (chain[k], chain[k + 1]);
            let (pa, pb) = (
                self.at(a).ok_or("a point is gone")?,
                self.at(b).ok_or("a point is gone")?,
            );
            let len = (pb - pa).norm();
            let ta = bends[k].as_ref().map_or(0.0, |x| x.0);
            let tb = bends[k + 1].as_ref().map_or(0.0, |x| x.0);
            if ta + tb > len + 1e-9 {
                return Err("Bend: the radius does not fit".into());
            }
            let s = bends[k].as_ref().map_or(pa, |x| x.3);
            let e = bends[k + 1].as_ref().map_or(pb, |x| x.2);
            if (e - s).norm() > 1e-9 {
                out.push(PathPiece::Line { a: s, b: e });
            }
            if (k + 1 < n || closed)
                && let Some((_, arc, _, _)) = bends[k + 1].clone()
            {
                out.push(arc);
            }
        }
        Ok(out)
    }

    /// Points along every line and bend (for drawing).
    pub fn polylines(&self) -> Vec<Vec<Point3>> {
        match self.pieces() {
            Ok(p) => vec![points_of(&p)],
            Err(_) => self
                .lines
                .iter()
                .filter_map(|l| Some(vec![self.at(l.a)?, self.at(l.b)?]))
                .collect(),
        }
    }

    /// The constraints' residuals at the current points.
    fn residuals(&self, at: &dyn Fn(u32) -> Option<Vector3>) -> Vec<Real> {
        let dir = |l: u32| -> Option<Vector3> {
            let l = self.line(l)?;
            (at(l.b)? - at(l.a)?).try_normalize(1e-12)
        };
        let mut r = Vec::new();
        for c in &self.constraints {
            match *c {
                Constraint3::Parallel(a, b) => {
                    if let (Some(x), Some(y)) = (dir(a), dir(b)) {
                        r.extend(x.cross(&y).iter().copied());
                    }
                }
                Constraint3::Perpendicular(a, b) => {
                    if let (Some(x), Some(y)) = (dir(a), dir(b)) {
                        r.push(x.dot(&y));
                    }
                }
            }
        }
        r
    }

    /// Solves the constraints by moving the free points as little as
    /// possible (damped least squares, numeric slopes); fails, leaving the
    /// sketch unchanged, when they cannot be met.
    pub fn solve(&mut self) -> Result<(), String> {
        use nalgebra::{DMatrix, DVector};
        if self.constraints.is_empty() {
            return Ok(());
        }
        let free: Vec<usize> = (0..self.points.len())
            .filter(|&i| !self.points[i].fixed)
            .collect();
        let start: Vec<Real> = free.iter().flat_map(|&i| self.points[i].at).collect();
        let scale = self
            .points
            .iter()
            .map(|p| v(p.at).norm())
            .fold(1.0, Real::max);
        let eval = |x: &[Real]| -> Vec<Real> {
            let at = |id: u32| -> Option<Vector3> {
                let i = self.points.iter().position(|p| p.id == id)?;
                Some(match free.iter().position(|&f| f == i) {
                    Some(k) => Vector3::new(x[3 * k], x[3 * k + 1], x[3 * k + 2]),
                    None => v(self.points[i].at),
                })
            };
            self.residuals(&at)
        };
        let mut x = start.clone();
        let mut r = eval(&x);
        let norm = |r: &[Real]| r.iter().map(|e| e * e).sum::<Real>().sqrt();
        let mut lambda = 1e-6;
        for _ in 0..200 {
            if norm(&r) < 1e-11 {
                break;
            }
            if x.is_empty() {
                break;
            }
            let (m, n) = (r.len(), x.len());
            let h = 1e-7 * scale;
            let mut j = DMatrix::zeros(m, n);
            for c in 0..n {
                let mut xp = x.clone();
                xp[c] += h;
                let rp = eval(&xp);
                for row in 0..m {
                    j[(row, c)] = (rp[row] - r[row]) / h;
                }
            }
            let rv = DVector::from_vec(r.clone());
            // Min-norm step: δ = −Jᵀ (J Jᵀ + λ I)⁻¹ r.
            let jjt = &j * j.transpose() + DMatrix::identity(m, m) * lambda;
            let Some(y) = jjt.lu().solve(&rv) else {
                lambda *= 10.0;
                continue;
            };
            let step = -(j.transpose() * y);
            let xn: Vec<Real> = x.iter().zip(step.iter()).map(|(a, d)| a + d).collect();
            let rn = eval(&xn);
            if norm(&rn) < norm(&r) {
                x = xn;
                r = rn;
                lambda = (lambda / 10.0).max(1e-12);
            } else {
                lambda *= 10.0;
                if lambda > 1e6 {
                    break;
                }
            }
        }
        if norm(&r) > 1e-9 {
            return Err("3D Sketch: the constraint cannot be met".into());
        }
        for (k, &i) in free.iter().enumerate() {
            self.points[i].at = [x[3 * k], x[3 * k + 1], x[3 * k + 2]];
        }
        // No line may have lost its length.
        for l in &self.lines {
            let (Some(a), Some(b)) = (self.at(l.a), self.at(l.b)) else {
                continue;
            };
            if (b - a).norm() < 1e-9 {
                return Err("3D Sketch: a line would lose its length".into());
            }
        }
        Ok(())
    }

    /// The length of the path (lines and bends), or of all lines when they
    /// make no single path.
    pub fn length(&self) -> Real {
        self.polylines()
            .iter()
            .map(|p| p.windows(2).map(|w| (w[1] - w[0]).norm()).sum::<Real>())
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    fn route(points: &[[Real; 3]]) -> Sketch3d {
        let mut s = Sketch3d::new("Sketch3D1", "3D Sketch1");
        let ids: Vec<u32> = points
            .iter()
            .map(|p| s.add_point(Point3::from(*p), false))
            .collect();
        for w in ids.windows(2) {
            s.add_line(w[0], w[1]).unwrap();
        }
        s
    }

    fn arc_length(p: &PathPiece) -> Real {
        match p {
            PathPiece::Line { a, b } => (b - a).norm(),
            PathPiece::Arc {
                a, center, angle, ..
            } => (a - center).norm() * angle.abs(),
            _ => 0.0,
        }
    }

    /// An L of two 100 mm lines with a bend of r 10: the lines lose r each
    /// (90°: tan 45° = 1), the arc is π r / 2 long and ends where the second
    /// line starts; a bend of r 150 does not fit.
    #[test]
    fn bend_of_a_right_angle() {
        for r in [1.0, 5.0, 10.0, 40.0, 99.0] {
            let mut s = route(&[[0.0, 0.0, 0.0], [100.0, 0.0, 0.0], [100.0, 0.0, 100.0]]);
            let corner = s.lines[0].b;
            s.add_bend(corner, r).unwrap();
            let p = s.pieces().unwrap();
            assert_eq!(p.len(), 3);
            let total: Real = p.iter().map(arc_length).sum();
            let want = 200.0 - 2.0 * r + PI * r / 2.0;
            assert!((total - want).abs() < 1e-9, "r {r}: {total} vs {want}");
            // The arc runs from the first line's end to the second's start.
            let PathPiece::Arc { .. } = &p[1] else {
                panic!("an arc in the middle")
            };
            let pts = points_of(&p[1..2]);
            let end = *pts.last().unwrap();
            assert!((end - Point3::new(100.0, 0.0, r)).norm() < 1e-6, "{end}");
        }
        let mut s = route(&[[0.0, 0.0, 0.0], [100.0, 0.0, 0.0], [100.0, 0.0, 100.0]]);
        let corner = s.lines[0].b;
        let before = s.clone();
        assert!(
            s.add_bend(corner, 150.0)
                .unwrap_err()
                .contains("does not fit")
        );
        assert_eq!(s, before);
    }

    /// A route through space (not in one plane) with three bends: each arc
    /// is tangent to both its lines (its end tangent along the next line).
    #[test]
    fn bends_of_a_space_route_are_tangent() {
        let mut s = route(&[
            [0.0, 0.0, 0.0],
            [100.0, 0.0, 0.0],
            [100.0, 80.0, 30.0],
            [40.0, 120.0, 90.0],
            [40.0, 120.0, 200.0],
        ]);
        let corners: Vec<u32> = (1..4).map(|k| s.lines[k].a).collect();
        for c in corners {
            s.add_bend(c, 12.0).unwrap();
        }
        let p = s.pieces().unwrap();
        assert_eq!(p.len(), 7);
        // Tangency: the direction leaving each arc equals the next line's.
        for k in (1..p.len()).step_by(2) {
            let pts = points_of(&p[k..k + 1]);
            let n = pts.len();
            let out = (pts[n - 1] - pts[n - 2]).normalize();
            let PathPiece::Line { a, b } = &p[k + 1] else {
                panic!("a line after the arc")
            };
            assert!((pts[n - 1] - a).norm() < 1e-6, "the arc ends on the line");
            let d = (b - a).normalize();
            assert!(out.dot(&d) > 0.999, "tangent: {out} vs {d}");
        }
    }

    /// Parallel and Perpendicular move the free points as little as needed;
    /// fixed points stay; an impossible one leaves the sketch unchanged.
    #[test]
    fn parallel_and_perpendicular() {
        let mut s = route(&[[0.0, 0.0, 0.0], [100.0, 0.0, 0.0], [110.0, 90.0, 20.0]]);
        let (l1, l2) = (s.lines[0].id, s.lines[1].id);
        s.points[0].fixed = true;
        s.points[1].fixed = true;
        s.add_constraint(Constraint3::Perpendicular(l1, l2))
            .unwrap();
        let (a, b, c) = (
            s.at(s.lines[0].a).unwrap(),
            s.at(s.lines[0].b).unwrap(),
            s.at(s.lines[1].b).unwrap(),
        );
        assert!(((b - a).normalize().dot(&(c - b).normalize())).abs() < 1e-9);
        assert_eq!(a, Point3::origin());
        // A third line parallel to the first.
        let d = s.add_point(Point3::new(150.0, 100.0, 30.0), false);
        let l3 = s.add_line(s.lines[1].b, d).unwrap();
        s.add_constraint(Constraint3::Parallel(l1, l3)).unwrap();
        let (c, d) = (s.at(s.line(l3).unwrap().a).unwrap(), s.at(d).unwrap());
        assert!((d - c).normalize().cross(&Vector3::x()).norm() < 1e-9);
        // Every point fixed and the lines not parallel: refused.
        let mut t = route(&[[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 10.0, 0.0]]);
        for p in &mut t.points {
            p.fixed = true;
        }
        let before = t.clone();
        let (a, b) = (t.lines[0].id, t.lines[1].id);
        assert!(t.add_constraint(Constraint3::Parallel(a, b)).is_err());
        assert_eq!(t, before);
    }

    /// Coincident joins two chains into one; branching is refused as a path.
    #[test]
    fn coincident_and_chains() {
        let mut s = route(&[[0.0, 0.0, 0.0], [50.0, 0.0, 0.0]]);
        let p = s.add_point(Point3::new(50.0, 0.0, 1.0), false);
        let q = s.add_point(Point3::new(50.0, 40.0, 1.0), false);
        s.add_line(p, q).unwrap();
        assert!(s.pieces().unwrap_err().contains("more than one chain"));
        let end = s.lines[0].b;
        s.merge_points(end, p).unwrap();
        assert_eq!(s.pieces().unwrap().len(), 2);
        assert_eq!(s.points.len(), 3);
        let r = s.add_point(Point3::new(50.0, 0.0, -30.0), false);
        s.add_line(end, r).unwrap();
        assert!(s.pieces().unwrap_err().contains("branches"));
    }

    /// Sweep of a Ø10 circle along 3D sketches with bends: a tube whose
    /// volume is π r² × the path length (Pappus over each bend); editing
    /// the 3D sketch rebuilds the sweep (the regeneration key follows it);
    /// a save and load builds the same.
    #[test]
    fn sweep_along_a_3d_sketch() {
        use crate::part::{
            OriginPlane, PartDocument, PathRef, ProfileRef, SketchOptions, SketchPlacement,
            SweepFeature,
        };
        let cases: [(&[[Real; 3]], Real); 5] = [
            (&[[0.0, 0.0, 0.0], [100.0, 0.0, 0.0]], 0.0),
            (
                &[[0.0, 0.0, 0.0], [100.0, 0.0, 0.0], [100.0, 0.0, 80.0]],
                20.0,
            ),
            (
                &[[0.0, 0.0, 0.0], [100.0, 0.0, 0.0], [100.0, 60.0, 0.0]],
                15.0,
            ),
            (
                &[
                    [0.0, 0.0, 0.0],
                    [80.0, 0.0, 0.0],
                    [80.0, 50.0, 30.0],
                    [80.0, 90.0, 120.0],
                ],
                12.0,
            ),
            (
                &[
                    [0.0, 0.0, 0.0],
                    [60.0, 0.0, 0.0],
                    [60.0, 70.0, 0.0],
                    [60.0, 70.0, 90.0],
                    [150.0, 70.0, 90.0],
                ],
                18.0,
            ),
        ];
        let r: Real = 5.0;
        for (points, bend) in cases {
            let mut part = PartDocument::default();
            let mut s3 = route(points);
            if bend > 0.0 {
                let corners: Vec<u32> = s3.lines.iter().skip(1).map(|l| l.a).collect();
                for c in corners {
                    s3.add_bend(c, bend).unwrap();
                }
            }
            let path_len = s3.length();
            let exact_len: Real = s3.pieces().unwrap().iter().map(arc_length).sum();
            part.sketches3d.push(s3);
            let opts = SketchOptions {
                auto_project_origin: false,
                auto_reference_edges: false,
                ..Default::default()
            };
            let i = part
                .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::YZ), &opts)
                .unwrap();
            part.sketches[i]
                .sketch
                .add_circle(crate::sketch::Point2::new(0.0, 0.0), r)
                .unwrap();
            let reg = part.sketches[i].sketch.regions();
            let pr = ProfileRef::of(&reg[0], reg[0].interior_point());
            let sw = SweepFeature::new(
                vec![pr],
                PathRef::Sketch3d {
                    sketch: "Sketch3D1".into(),
                },
            );
            part.add_sweep(i, sw).unwrap();
            assert!(part.feature_health.is_empty(), "{:?}", part.feature_health);
            let vol: Real = part
                .exact_bodies
                .0
                .iter()
                .map(|b| b.measure_full().unwrap().volume)
                .sum();
            let want = PI * r * r * exact_len;
            assert!(
                (vol - want).abs() < 2e-3 * want,
                "{points:?} bend {bend}: {vol} vs {want} (path {path_len})"
            );
            // Moving the last point rebuilds the sweep.
            let last = part.sketches3d[0].points.len() - 1;
            part.sketches3d[0].points[last].at[0] += 30.0;
            part.regenerate().unwrap();
            let longer: Real = part.sketches3d[0]
                .pieces()
                .unwrap()
                .iter()
                .map(arc_length)
                .sum();
            let vol2: Real = part
                .exact_bodies
                .0
                .iter()
                .map(|b| b.measure_full().unwrap().volume)
                .sum();
            assert!(
                (vol2 - PI * r * r * longer).abs() < 2e-3 * vol2,
                "edited: {vol2} vs {}",
                PI * r * r * longer
            );
            let text = serde_json::to_string(&part).unwrap();
            let mut back: PartDocument = serde_json::from_str(&text).unwrap();
            back.regenerate().unwrap();
            assert_eq!(back.sketches3d, part.sketches3d);
            let vol3: Real = back
                .exact_bodies
                .0
                .iter()
                .map(|b| b.measure_full().unwrap().volume)
                .sum();
            assert!((vol3 - vol2).abs() < 1e-6 * vol2);
        }
    }

    #[test]
    fn serde_round_trip() {
        let mut s = route(&[[0.0, 0.0, 0.0], [100.0, 0.0, 0.0], [100.0, 50.0, 0.0]]);
        let c = s.lines[0].b;
        s.add_bend(c, 5.0).unwrap();
        let (a, b) = (s.lines[0].id, s.lines[1].id);
        s.add_constraint(Constraint3::Perpendicular(a, b)).unwrap();
        let text = serde_json::to_string(&s).unwrap();
        let back: Sketch3d = serde_json::from_str(&text).unwrap();
        assert_eq!(back, s);
    }
}
