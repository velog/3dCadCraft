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

/// A three-point arc in space: from `a` through `via` to `b`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Arc3d {
    /// Its id in the sketch.
    pub id: u32,
    /// Start point id.
    pub a: u32,
    /// End point id.
    pub b: u32,
    /// A point the arc passes through (between the ends).
    pub via: u32,
}

/// How a spline uses its points.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplineKind {
    /// Through every point (chord-length cubic, C1).
    #[default]
    Interpolation,
    /// The points are the control vertices (clamped cubic B-spline).
    ControlVertex,
}

/// A spline in space through (or controlled by) points of the sketch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Spline3d {
    /// Its id in the sketch.
    pub id: u32,
    /// Its points, in order (the first and last are its ends).
    pub points: Vec<u32>,
    /// Interpolation or control vertex.
    #[serde(default)]
    pub kind: SplineKind,
}

/// A helical curve (Inventor: Helical Curve): about an axis through point
/// `center` along `axis`, starting at point `start` (its distance from the
/// axis is the radius); `end` is a point of the sketch the helix places at
/// its far end.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Helix3d {
    /// Its id in the sketch.
    pub id: u32,
    /// A point on the axis (the base).
    pub center: u32,
    /// The axis direction (unit, part coordinates).
    pub axis: [Real; 3],
    /// The start point (radius and start angle).
    pub start: u32,
    /// The end point (kept where the helix ends).
    pub end: u32,
    /// Rise per turn (mm, > 0).
    pub pitch: Real,
    /// Turns (> 0, need not be whole).
    pub turns: Real,
    /// Turns clockwise seen from the axis tip (Inventor's CW); off: counter-clockwise.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub clockwise: bool,
}

impl Helix3d {
    /// (axis point at the start's height, unit axis, unit radial at the
    /// start, radius, signed turning sense).
    fn frame(&self, sk: &Sketch3d) -> Result<(Point3, Vector3, Vector3, Real, Real), String> {
        let c = sk
            .at(self.center)
            .ok_or("Helical Curve: its centre point is gone")?;
        let s = sk
            .at(self.start)
            .ok_or("Helical Curve: its start point is gone")?;
        let ax = v(self.axis)
            .try_normalize(1e-12)
            .ok_or("Helical Curve: the axis has no direction")?;
        let h = (s - c).dot(&ax);
        let base = c + ax * h;
        let rv = s - base;
        let r = rv.norm();
        if r < 1e-9 {
            return Err("Helical Curve: the start point lies on the axis".into());
        }
        if !(self.pitch > 0.0
            && self.pitch.is_finite()
            && self.turns > 0.0
            && self.turns.is_finite())
        {
            return Err("Helical Curve: the pitch and the turns must be above 0".into());
        }
        Ok((base, ax, rv / r, r, if self.clockwise { -1.0 } else { 1.0 }))
    }

    /// Where the helix ends.
    pub fn end_point(&self, sk: &Sketch3d) -> Result<Point3, String> {
        let (base, ax, u, r, sense) = self.frame(sk)?;
        let w = ax.cross(&u) * sense;
        let t = std::f64::consts::TAU * self.turns;
        Ok(base + (u * t.cos() + w * t.sin()) * r + ax * (self.pitch * self.turns))
    }

    /// The helix as cubic Bézier spans of at most a quarter turn each
    /// (the circular part with the 4/3·tan(θ/4) handles, the rise linear).
    pub fn piece(&self, sk: &Sketch3d) -> Result<PathPiece, String> {
        let (base, ax, u, r, sense) = self.frame(sk)?;
        let w = ax.cross(&u) * sense;
        let total = std::f64::consts::TAU * self.turns;
        let n = (self.turns * 4.0).ceil().max(1.0) as usize;
        let dt = total / n as Real;
        let rise = self.pitch / std::f64::consts::TAU; // per radian
        let at = |t: Real| base + (u * t.cos() + w * t.sin()) * r + ax * (rise * t);
        let tan = |t: Real| (w * t.cos() - u * t.sin()) * r;
        let k = 4.0 / 3.0 * (dt / 4.0).tan();
        let spans: Vec<Vec<Point3>> = (0..n)
            .map(|i| {
                let (t0, t1) = (dt * i as Real, dt * (i + 1) as Real);
                vec![
                    at(t0),
                    at(t0) + tan(t0) * k + ax * (rise * dt / 3.0),
                    at(t1) - tan(t1) * k - ax * (rise * dt / 3.0),
                    at(t1),
                ]
            })
            .collect();
        let mut points = Vec::new();
        for s in &spans {
            bezier_points(s, &mut points, 16);
        }
        Ok(PathPiece::Spline { points, spans })
    }
}

/// One chain of curves: (curve, from point, to point) in order.
pub type Chain3 = Vec<(SegId, u32, u32)>;

/// A curve of the sketch that can be part of a chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegId {
    /// A line.
    Line(u32),
    /// An arc.
    Arc(u32),
    /// A spline.
    Spline(u32),
    /// A helical curve.
    Helix(u32),
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
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Constraint3 {
    /// The two lines run the same way (or opposite).
    Parallel(u32, u32),
    /// The two lines are square to each other.
    Perpendicular(u32, u32),
    /// The line runs along an axis of the part (0 = X, 1 = Y, 2 = Z).
    AlongAxis(u32, usize),
    /// The two lines are equally long.
    Equal(u32, u32),
    /// Dimension: the line's length (mm).
    Length(u32, Real),
    /// Dimension: the angle between two lines' directions (radians).
    Angle(u32, u32, Real),
    /// Dimension: the distance between two points (mm).
    Distance(u32, u32, Real),
}

impl Constraint3 {
    /// The lines it names.
    pub fn lines(&self) -> Vec<u32> {
        match *self {
            Constraint3::Parallel(a, b)
            | Constraint3::Perpendicular(a, b)
            | Constraint3::Equal(a, b)
            | Constraint3::Angle(a, b, _) => vec![a, b],
            Constraint3::AlongAxis(a, _) | Constraint3::Length(a, _) => vec![a],
            Constraint3::Distance(..) => Vec::new(),
        }
    }

    /// The points it names.
    pub fn points(&self) -> Vec<u32> {
        match *self {
            Constraint3::Distance(a, b, _) => vec![a, b],
            _ => Vec::new(),
        }
    }

    /// Its value, for a dimension.
    pub fn value(&self) -> Option<Real> {
        match *self {
            Constraint3::Length(_, v)
            | Constraint3::Angle(_, _, v)
            | Constraint3::Distance(_, _, v) => Some(v),
            _ => None,
        }
    }

    /// Its name in the panel.
    pub fn label(&self) -> String {
        match *self {
            Constraint3::Parallel(..) => "Parallel".into(),
            Constraint3::Perpendicular(..) => "Perpendicular".into(),
            Constraint3::AlongAxis(_, k) => {
                format!("Parallel to {}", ["X", "Y", "Z"].get(k).unwrap_or(&"?"))
            }
            Constraint3::Equal(..) => "Equal".into(),
            Constraint3::Length(_, v) => format!("Length {v:.3} mm"),
            Constraint3::Angle(_, _, v) => format!("Angle {:.3}°", v.to_degrees()),
            Constraint3::Distance(_, _, v) => format!("Distance {v:.3} mm"),
        }
    }
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
    /// Its arcs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arcs: Vec<Arc3d>,
    /// Its splines.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub splines: Vec<Spline3d>,
    /// Its helical curves.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub helices: Vec<Helix3d>,
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
            arcs: Vec::new(),
            splines: Vec::new(),
            helices: Vec::new(),
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

    /// Adds a three-point arc (start, a point on it, end); returns its id.
    pub fn add_arc(&mut self, a: u32, via: u32, b: u32) -> Result<u32, String> {
        let (Some(pa), Some(pv), Some(pb)) = (self.at(a), self.at(via), self.at(b)) else {
            return Err("Arc: its points are not in the sketch".into());
        };
        if circle_through(pa, pv, pb).is_none() {
            return Err("Arc: the three points lie on one line".into());
        }
        let id = self.take_id();
        self.arcs.push(Arc3d { id, a, b, via });
        Ok(id)
    }

    /// Adds a spline through (or controlled by) `points` (at least 2,
    /// neighbours apart); returns its id.
    pub fn add_spline(&mut self, points: Vec<u32>, kind: SplineKind) -> Result<u32, String> {
        if points.len() < 2 {
            return Err("Spline: it needs at least 2 points".into());
        }
        for w in points.windows(2) {
            let (Some(p), Some(q)) = (self.at(w[0]), self.at(w[1])) else {
                return Err("Spline: a point is not in the sketch".into());
            };
            if (p - q).norm() < 1e-9 {
                return Err("Spline: two neighbouring points coincide".into());
            }
        }
        let id = self.take_id();
        self.splines.push(Spline3d { id, points, kind });
        Ok(id)
    }

    /// Adds a helical curve about the axis through `center` along `axis`
    /// from `start`; its end point is made (fixed: the helix places it).
    #[allow(clippy::too_many_arguments)]
    pub fn add_helix(
        &mut self,
        center: u32,
        axis: Vector3,
        start: u32,
        pitch: Real,
        turns: Real,
        clockwise: bool,
    ) -> Result<u32, String> {
        let mut h = Helix3d {
            id: 0,
            center,
            axis: [axis.x, axis.y, axis.z],
            start,
            end: 0,
            pitch,
            turns,
            clockwise,
        };
        let e = h.end_point(self)?;
        let id = self.take_id();
        let end = self.take_id();
        self.points.push(Point3d {
            id: end,
            at: [e.x, e.y, e.z],
            fixed: true,
        });
        h.id = id;
        h.end = end;
        self.helices.push(h);
        Ok(id)
    }

    /// Moves every helix's end point to where its helix ends now.
    pub fn refresh_helices(&mut self) {
        let ends: Vec<(u32, Option<Point3>)> = self
            .helices
            .iter()
            .map(|h| (h.end, h.end_point(self).ok()))
            .collect();
        for (id, at) in ends {
            if let (Some(at), Some(p)) = (at, self.points.iter_mut().find(|p| p.id == id)) {
                p.at = [at.x, at.y, at.z];
            }
        }
    }

    /// Fix (or free) point `p`.
    pub fn set_fixed(&mut self, p: u32, on: bool) -> Result<(), String> {
        let q = self
            .points
            .iter_mut()
            .find(|q| q.id == p)
            .ok_or("the point is not in the sketch")?;
        q.fixed = on;
        Ok(())
    }

    /// **Mirror** (Inventor's 3D sketch Mirror): copies of the curves whose
    /// points are all in `points` (and of the bends at their corners),
    /// reflected in the plane through `origin` with normal `normal`. A point
    /// on the plane is shared with its copy; a copy keeps its point's Fix.
    /// Helical curves are left (a mirror turns their hand). Returns the new
    /// points.
    pub fn mirror(
        &mut self,
        points: &[u32],
        origin: Point3,
        normal: Vector3,
    ) -> Result<Vec<u32>, String> {
        let n = normal
            .try_normalize(1e-12)
            .ok_or("Mirror: the plane has no normal")?;
        let reflect = |p: Point3| p - n * (2.0 * (p - origin).dot(&n));
        let has = |id: &u32| points.contains(id);
        let lines: Vec<(u32, u32)> = self
            .lines
            .iter()
            .filter(|l| has(&l.a) && has(&l.b))
            .map(|l| (l.a, l.b))
            .collect();
        let arcs: Vec<(u32, u32, u32)> = self
            .arcs
            .iter()
            .filter(|a| has(&a.a) && has(&a.b) && has(&a.via))
            .map(|a| (a.a, a.via, a.b))
            .collect();
        let splines: Vec<(Vec<u32>, SplineKind)> = self
            .splines
            .iter()
            .filter(|s| s.points.iter().all(has))
            .map(|s| (s.points.clone(), s.kind))
            .collect();
        if lines.is_empty() && arcs.is_empty() && splines.is_empty() && points.is_empty() {
            return Err("Mirror: select the curves to mirror".into());
        }
        let mut map: Vec<(u32, u32)> = Vec::new();
        for &id in points {
            let p = self
                .point(id)
                .ok_or("Mirror: a point is not in the sketch")?;
            let (at, fixed) = (Point3::from(p.at), p.fixed);
            let q = reflect(at);
            let copy = if (q - at).norm() < 1e-9 {
                id
            } else {
                self.add_point(q, fixed)
            };
            map.push((id, copy));
        }
        let to = |id: u32| map.iter().find(|(a, _)| *a == id).map_or(id, |(_, b)| *b);
        for (a, b) in lines {
            if to(a) != a || to(b) != b {
                self.add_line(to(a), to(b))?;
            }
        }
        for (a, via, b) in arcs {
            self.add_arc(to(a), to(via), to(b))?;
        }
        for (pts, kind) in splines {
            self.add_spline(pts.into_iter().map(to).collect(), kind)?;
        }
        let bends: Vec<Bend3d> = self
            .bends
            .iter()
            .filter(|b| has(&b.corner) && to(b.corner) != b.corner)
            .cloned()
            .collect();
        for b in bends {
            self.add_bend(to(b.corner), b.radius)?;
        }
        Ok(map.into_iter().map(|(_, b)| b).collect())
    }

    /// Removes constraint `k`.
    pub fn remove_constraint(&mut self, k: usize) {
        if k < self.constraints.len() {
            self.constraints.remove(k);
        }
    }

    /// **Include Geometry** of a model edge given by points along it: a
    /// straight edge (2 points) becomes a line, a circular one (3 points:
    /// ends and middle) an arc, any other an interpolation spline through
    /// the points. Its points are fixed (a copy, not associative).
    pub fn include(&mut self, pts: &[Point3], circular: bool) -> Result<SegId, String> {
        if pts.len() < 2 {
            return Err("Include Geometry: the edge has no length".into());
        }
        let ids: Vec<u32> = pts.iter().map(|p| self.add_point(*p, true)).collect();
        if ids.len() == 2 {
            return self.add_line(ids[0], ids[1]).map(SegId::Line);
        }
        if circular && ids.len() == 3 {
            return self.add_arc(ids[0], ids[1], ids[2]).map(SegId::Arc);
        }
        self.add_spline(ids, SplineKind::Interpolation)
            .map(SegId::Spline)
    }

    /// Every curve with its two end points.
    pub fn segs(&self) -> Vec<(SegId, u32, u32)> {
        let mut v: Vec<(SegId, u32, u32)> = self
            .lines
            .iter()
            .map(|l| (SegId::Line(l.id), l.a, l.b))
            .collect();
        v.extend(self.arcs.iter().map(|a| (SegId::Arc(a.id), a.a, a.b)));
        v.extend(
            self.splines
                .iter()
                .filter_map(|s| Some((SegId::Spline(s.id), *s.points.first()?, *s.points.last()?))),
        );
        v.extend(
            self.helices
                .iter()
                .map(|h| (SegId::Helix(h.id), h.start, h.end)),
        );
        v
    }

    /// The curves ending at point `p`.
    pub fn segs_at(&self, p: u32) -> Vec<(SegId, u32, u32)> {
        self.segs()
            .into_iter()
            .filter(|(_, a, b)| *a == p || *b == p)
            .collect()
    }

    /// The path piece of curve `seg` run from its end `from`.
    pub fn seg_piece(&self, seg: SegId, from: u32) -> Result<PathPiece, String> {
        let gone = || "a curve's point is gone".to_owned();
        let at = |id: u32| self.at(id).ok_or_else(gone);
        Ok(match seg {
            SegId::Line(id) => {
                let l = self.line(id).ok_or_else(gone)?;
                let (a, b) = (at(l.a)?, at(l.b)?);
                let (a, b) = if from == l.a { (a, b) } else { (b, a) };
                PathPiece::Line { a, b }
            }
            SegId::Arc(id) => {
                let c = self.arcs.iter().find(|c| c.id == id).ok_or_else(gone)?;
                let arc = arc_piece(at(c.a)?, at(c.via)?, at(c.b)?)
                    .ok_or("Arc: the three points lie on one line")?;
                if from == c.a { arc } else { reversed(&arc) }
            }
            SegId::Helix(id) => {
                let h = self.helices.iter().find(|c| c.id == id).ok_or_else(gone)?;
                let piece = h.piece(self)?;
                if from == h.start {
                    piece
                } else {
                    reversed(&piece)
                }
            }
            SegId::Spline(id) => {
                let sp = self.splines.iter().find(|c| c.id == id).ok_or_else(gone)?;
                let pts: Vec<Point3> =
                    sp.points.iter().map(|&p| at(p)).collect::<Result<_, _>>()?;
                let piece = spline_piece(&pts, sp.kind)?;
                if sp.points.first() == Some(&from) {
                    piece
                } else {
                    reversed(&piece)
                }
            }
        })
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
        for c in &mut next.arcs {
            for p in [&mut c.a, &mut c.b, &mut c.via] {
                if *p == drop {
                    *p = keep;
                }
            }
        }
        for sp in &mut next.splines {
            for p in &mut sp.points {
                if *p == drop {
                    *p = keep;
                }
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
        let lines = c.lines();
        if lines.iter().any(|&l| self.line(l).is_none())
            || (lines.len() == 2 && lines[0] == lines[1])
        {
            return Err("pick lines of the sketch".into());
        }
        let points = c.points();
        if points.iter().any(|&p| self.point(p).is_none())
            || (points.len() == 2 && points[0] == points[1])
        {
            return Err("pick two points of the sketch".into());
        }
        if c.value().is_some_and(|v| !(v.is_finite() && v >= 0.0)) {
            return Err("a dimension must be a number of 0 or more".into());
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
        let at = self.segs_at(corner);
        if at.len() != 2 || at.iter().any(|(s, ..)| !matches!(s, SegId::Line(_))) {
            return Err("Bend: pick a point where two lines meet".into());
        }

        let mut next = self.clone();
        next.bends.retain(|b| b.corner != corner);
        next.bends.push(Bend3d { corner, radius });
        // Every chain must still fit its bends (a sketch may hold several
        // chains: a mirror, included edges).
        for chain in next.chains()? {
            next.chain_pieces(&chain)?;
        }
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

    /// The bend at `corner` between line `into` (running into the corner)
    /// and line `out`: (trim length along each line, the arc, its start and
    /// end) or `None` when there is none.
    #[allow(clippy::type_complexity)]
    fn bend_at(
        &self,
        corner: u32,
        into: u32,
        out: u32,
    ) -> Result<Option<(Real, PathPiece, Point3, Point3)>, String> {
        let Some(b) = self.bends.iter().find(|b| b.corner == corner) else {
            return Ok(None);
        };
        let (Some(l1), Some(l2)) = (self.line(into), self.line(out)) else {
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

    /// The chains of curves: each a list of (curve, from point, to point)
    /// from an end (or round a closed loop). A point where three curves
    /// meet is refused.
    pub fn chains(&self) -> Result<Vec<Chain3>, String> {
        let segs = self.segs();
        let degree = |p: u32| segs.iter().filter(|(_, a, b)| *a == p || *b == p).count();
        if self.points.iter().any(|p| degree(p.id) > 2) {
            return Err("the path branches at a point".into());
        }
        let mut used: Vec<SegId> = Vec::new();
        let mut out = Vec::new();
        let mut starts: Vec<u32> = self
            .points
            .iter()
            .filter(|p| degree(p.id) == 1)
            .map(|p| p.id)
            .collect();
        starts.extend(
            self.points
                .iter()
                .filter(|p| degree(p.id) == 2)
                .map(|p| p.id),
        );
        for s in starts {
            let mut at = s;
            let mut chain = Vec::new();
            while let Some(&(id, a, b)) = segs
                .iter()
                .find(|(id, a, b)| !used.contains(id) && (*a == at || *b == at))
            {
                used.push(id);
                let to = if a == at { b } else { a };
                chain.push((id, at, to));
                at = to;
            }
            if !chain.is_empty() {
                out.push(chain);
            }
        }
        Ok(out)
    }

    /// The sketch as one path: its curves in chain order, lines shortened
    /// by the bends, and the bends' arcs. Refused when the curves make more
    /// than one chain or branch, or a bend does not fit.
    pub fn pieces(&self) -> Result<Vec<PathPiece>, String> {
        let chains = self.chains()?;
        let Some(chain) = chains.first() else {
            return Err("the 3D sketch has no curves".into());
        };
        if chains.len() > 1 {
            return Err("the 3D sketch has more than one chain of curves".into());
        }
        self.chain_pieces(chain)
    }

    /// One chain's curves as path pieces: lines shortened by the bends, the
    /// bends' arcs between them.
    pub fn chain_pieces(&self, chain: &Chain3) -> Result<Vec<PathPiece>, String> {
        let n = chain.len();
        let closed = n > 1 && chain[0].1 == chain[n - 1].2;
        // bends[k]: the bend at the start of curve k (between k - 1 and k).
        let mut bends = Vec::with_capacity(n + 1);
        for k in 0..=n {
            let pair = if k > 0 && k < n {
                Some((chain[k - 1], chain[k]))
            } else if closed {
                Some((chain[n - 1], chain[0]))
            } else {
                None
            };
            bends.push(match pair {
                Some(((SegId::Line(i), _, c), (SegId::Line(o), ..))) => self.bend_at(c, i, o)?,
                _ => None,
            });
        }
        let mut out = Vec::new();
        for (k, &(seg, from, _)) in chain.iter().enumerate() {
            match self.seg_piece(seg, from)? {
                PathPiece::Line { a: pa, b: pb } => {
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
                }
                piece => out.push(piece),
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
        let per_chain = self.chains().and_then(|cs| {
            cs.iter()
                .map(|c| self.chain_pieces(c).map(|p| points_of(&p)))
                .collect::<Result<Vec<_>, String>>()
        });
        match per_chain {
            Ok(p) => p,
            Err(_) => self
                .segs()
                .into_iter()
                .filter_map(|(id, a, _)| Some(points_of(&[self.seg_piece(id, a).ok()?])))
                .collect(),
        }
    }

    /// The constraints' residuals at the current points.
    fn residuals(&self, at: &dyn Fn(u32) -> Option<Vector3>) -> Vec<Real> {
        let dir = |l: u32| -> Option<Vector3> {
            let l = self.line(l)?;
            (at(l.b)? - at(l.a)?).try_normalize(1e-12)
        };
        let len = |l: u32| -> Option<Real> {
            let l = self.line(l)?;
            Some((at(l.b)? - at(l.a)?).norm())
        };
        // Lengths in residuals are relative to the sketch's size, like the
        // direction terms.
        let scale = self
            .points
            .iter()
            .map(|p| v(p.at).norm())
            .fold(1.0, Real::max);
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
                Constraint3::AlongAxis(a, k) => {
                    if let Some(x) = dir(a) {
                        let mut ax = Vector3::zeros();
                        ax[k.min(2)] = 1.0;
                        r.extend(x.cross(&ax).iter().copied());
                    }
                }
                Constraint3::Equal(a, b) => {
                    if let (Some(x), Some(y)) = (len(a), len(b)) {
                        r.push((x - y) / scale);
                    }
                }
                Constraint3::Length(a, v) => {
                    if let Some(x) = len(a) {
                        r.push((x - v) / scale);
                    }
                }
                Constraint3::Angle(a, b, v) => {
                    if let (Some(x), Some(y)) = (dir(a), dir(b)) {
                        r.push(x.cross(&y).norm().atan2(x.dot(&y)) - v);
                    }
                }
                Constraint3::Distance(a, b, v) => {
                    if let (Some(x), Some(y)) = (at(a), at(b)) {
                        r.push(((y - x).norm() - v) / scale);
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
        self.refresh_helices();
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

/// The circle through three points: centre, unit normal (a, v, b turn
/// right-handed about it), radius.
fn circle_through(a: Point3, v: Point3, b: Point3) -> Option<(Point3, Vector3, Real)> {
    let (u, w) = (v - a, b - a);
    let n = u.cross(&w);
    let nn = n.norm_squared();
    if nn < 1e-18 * (u.norm_squared() * w.norm_squared()).max(1e-30) {
        return None;
    }
    // Circumcentre: a + (|u|² (w × n) + |w|² (n × u)) / (2 |n|²).
    let c = a + (w.cross(&n) * u.norm_squared() + n.cross(&u) * w.norm_squared()) / (2.0 * nn);
    Some((c, n / nn.sqrt(), (a - c).norm()))
}

/// The arc from `a` through `v` to `b` as a path piece.
fn arc_piece(a: Point3, v: Point3, b: Point3) -> Option<PathPiece> {
    let (c, n, _) = circle_through(a, v, b)?;
    let ra = a - c;
    // Angles about n (right hand), from a, in [0, 2π).
    let ang = |r: Vector3| {
        let y = n.cross(&ra);
        let t = r.dot(&y).atan2(r.dot(&ra));
        if t < 0.0 {
            t + std::f64::consts::TAU
        } else {
            t
        }
    };
    let (tv, tb) = (ang(v - c), ang(b - c));
    // Turning about n from a reaches v before b: that way round; else the
    // other way (about -n).
    let (axis, angle) = if tv <= tb {
        (n, tb)
    } else {
        (-n, std::f64::consts::TAU - tb)
    };
    Some(PathPiece::Arc {
        a,
        center: c,
        axis,
        angle,
    })
}

/// A piece run the other way.
fn reversed(p: &PathPiece) -> PathPiece {
    match p {
        PathPiece::Line { a, b } => PathPiece::Line { a: *b, b: *a },
        PathPiece::Arc {
            a,
            center,
            axis,
            angle,
        } => {
            let r =
                nalgebra::Rotation3::from_axis_angle(&nalgebra::Unit::new_normalize(*axis), *angle);
            PathPiece::Arc {
                a: center + r * (a - center),
                center: *center,
                axis: -axis,
                angle: *angle,
            }
        }
        PathPiece::Points(v) => PathPiece::Points(v.iter().rev().copied().collect()),
        PathPiece::Spline { points, spans } => PathPiece::Spline {
            points: points.iter().rev().copied().collect(),
            spans: spans
                .iter()
                .rev()
                .map(|s| s.iter().rev().copied().collect())
                .collect(),
        },
    }
}

/// Points along a cubic Bézier span (`n` steps), appended without
/// repeating the joint.
fn bezier_points(c: &[Point3], out: &mut Vec<Point3>, n: usize) {
    for i in 0..=n {
        if i == 0 && !out.is_empty() {
            continue;
        }
        let t = i as Real / n as Real;
        let mt = 1.0 - t;
        let p = c[0].coords * (mt * mt * mt)
            + c[1].coords * (3.0 * mt * mt * t)
            + c[2].coords * (3.0 * mt * t * t)
            + c[3].coords * (t * t * t);
        out.push(Point3::from(p));
    }
}

/// A spline as a path piece: cubic Bézier spans (exact for the kernel) and
/// points along them. Interpolation: through every point, the inner
/// tangents from the neighbours by chord length (Catmull-Rom), the ends'
/// those of the parabola through the first / last three points. Control
/// vertex: a clamped uniform cubic B-spline of the points.
fn spline_piece(pts: &[Point3], kind: SplineKind) -> Result<PathPiece, String> {
    let n = pts.len();
    let chord = |i: usize| (pts[i + 1] - pts[i]).norm();
    let spans: Vec<Vec<Point3>> = match kind {
        SplineKind::Interpolation => {
            let inner = |i: usize| -> Vector3 {
                let (d0, d1) = (chord(i - 1), chord(i));
                ((pts[i] - pts[i - 1]) / d0 * d1 + (pts[i + 1] - pts[i]) / d1 * d0).normalize()
            };
            let tangent = |i: usize| -> Vector3 {
                if n == 2 {
                    (pts[1] - pts[0]) / chord(0)
                } else if i == 0 {
                    ((pts[1] - pts[0]) / chord(0) * 2.0 - inner(1)).normalize()
                } else if i == n - 1 {
                    ((pts[n - 1] - pts[n - 2]) / chord(n - 2) * 2.0 - inner(n - 2)).normalize()
                } else {
                    inner(i)
                }
            };
            (0..n - 1)
                .map(|i| {
                    let h = chord(i);
                    vec![
                        pts[i],
                        pts[i] + tangent(i) * (h / 3.0),
                        pts[i + 1] - tangent(i + 1) * (h / 3.0),
                        pts[i + 1],
                    ]
                })
                .collect()
        }
        SplineKind::ControlVertex if n == 2 => vec![vec![
            pts[0],
            pts[0] + (pts[1] - pts[0]) / 3.0,
            pts[0] + (pts[1] - pts[0]) * (2.0 / 3.0),
            pts[1],
        ]],
        // A quadratic Bézier raised to a cubic.
        SplineKind::ControlVertex if n == 3 => vec![vec![
            pts[0],
            pts[0] + (pts[1] - pts[0]) * (2.0 / 3.0),
            pts[2] + (pts[1] - pts[2]) * (2.0 / 3.0),
            pts[2],
        ]],
        SplineKind::ControlVertex => {
            let b = crate::kernel::BSplineCurve::sketch_spline(pts.to_vec(), &[])
                .map_err(|e| format!("Spline: {e}"))?;
            crate::kernel::exact::spline_spans(b.degree(), b.knots(), b.control_points())?
        }
    };
    let mut points = Vec::new();
    for s in &spans {
        bezier_points(s, &mut points, 24);
    }
    Ok(PathPiece::Spline { points, spans })
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

    fn length_of(p: &[PathPiece]) -> Real {
        let pts = points_of(p);
        pts.windows(2).map(|w| (w[1] - w[0]).norm()).sum()
    }

    /// Three-point arcs: a quarter circle (both ways round, started from
    /// either end), a half circle through its top, an arc in a tilted
    /// plane; three points on a line are refused.
    #[test]
    fn three_point_arcs() {
        let r = 10.0;
        let h = r * std::f64::consts::FRAC_1_SQRT_2;
        type Case = ([Real; 3], [Real; 3], [Real; 3], Real);
        let cases: [Case; 4] = [
            ([r, 0.0, 0.0], [h, h, 0.0], [0.0, r, 0.0], PI * r / 2.0),
            ([0.0, r, 0.0], [h, h, 0.0], [r, 0.0, 0.0], PI * r / 2.0),
            ([r, 0.0, 0.0], [0.0, r, 0.0], [-r, 0.0, 0.0], PI * r),
            // Three quarters: from (r, 0) round through (0, -r) to (0, r).
            ([r, 0.0, 0.0], [0.0, -r, 0.0], [0.0, r, 0.0], 1.5 * PI * r),
        ];
        for (a, v, b, want) in cases {
            for tilt in [0.0, 0.7] {
                let rot = nalgebra::Rotation3::from_axis_angle(&Vector3::x_axis(), tilt);
                let p = |q: [Real; 3]| {
                    Point3::from(rot * Vector3::from(q)) + Vector3::new(5.0, 1.0, 2.0)
                };
                let mut s = Sketch3d::new("Sketch3D1", "3D Sketch1");
                let (ia, iv, ib) = (
                    s.add_point(p(a), false),
                    s.add_point(p(v), false),
                    s.add_point(p(b), false),
                );
                s.add_arc(ia, iv, ib).unwrap();
                let pieces = s.pieces().unwrap();
                let PathPiece::Arc { angle, .. } = &pieces[0] else {
                    panic!("an arc")
                };
                assert!(
                    (angle * r - want).abs() < 1e-9,
                    "{a:?} {v:?} {b:?}: {}",
                    angle * r
                );
                // The drawn points pass through `via` and end at the ends.
                let pts = points_of(&pieces);
                let near = |q: Point3| {
                    pts.windows(2)
                        .map(|w| {
                            let d = w[1] - w[0];
                            let t =
                                ((q - w[0]).dot(&d) / d.norm_squared().max(1e-30)).clamp(0.0, 1.0);
                            (w[0] + d * t - q).norm()
                        })
                        .fold(Real::MAX, Real::min)
                };
                assert!(near(p(v)) < 0.02, "through via");
                let (first, last) = (pts[0], pts[pts.len() - 1]);
                assert!(
                    ((first - p(a)).norm() < 1e-6 && (last - p(b)).norm() < 1e-6)
                        || ((first - p(b)).norm() < 1e-6 && (last - p(a)).norm() < 1e-6)
                );
                assert!((length_of(&pieces) - want).abs() < 2e-3 * want);
            }
        }
        let mut s = Sketch3d::new("Sketch3D1", "3D Sketch1");
        let ids: Vec<u32> = [[0.0, 0.0, 0.0], [1.0, 1.0, 1.0], [2.0, 2.0, 2.0]]
            .iter()
            .map(|q| s.add_point(Point3::from(*q), false))
            .collect();
        assert!(
            s.add_arc(ids[0], ids[1], ids[2])
                .unwrap_err()
                .contains("one line")
        );
    }

    /// Splines: an interpolation spline passes through every point and is
    /// tangent to its spans at the joints (C1); a control-vertex spline
    /// starts and ends at its first and last points, tangent to the first
    /// and last legs; both chain with lines (a line, a spline, a line).
    #[test]
    fn splines_through_and_by_points() {
        let pts: [[Real; 3]; 5] = [
            [0.0, 0.0, 0.0],
            [30.0, 10.0, 5.0],
            [60.0, -5.0, 20.0],
            [80.0, 20.0, 40.0],
            [100.0, 30.0, 35.0],
        ];
        for kind in [SplineKind::Interpolation, SplineKind::ControlVertex] {
            for n in 2..=5 {
                let mut s = Sketch3d::new("Sketch3D1", "3D Sketch1");
                let ids: Vec<u32> = pts[..n]
                    .iter()
                    .map(|q| s.add_point(Point3::from(*q), false))
                    .collect();
                s.add_spline(ids.clone(), kind).unwrap();
                let pieces = s.pieces().unwrap();
                let PathPiece::Spline { points, spans } = &pieces[0] else {
                    panic!("a spline")
                };
                assert!((points[0] - Point3::from(pts[0])).norm() < 1e-9);
                assert!((points[points.len() - 1] - Point3::from(pts[n - 1])).norm() < 1e-9);
                if kind == SplineKind::Interpolation {
                    assert_eq!(spans.len(), n - 1);
                    for (k, q) in pts[..n].iter().enumerate() {
                        let hit = points.iter().any(|x| (x - Point3::from(*q)).norm() < 1e-9);
                        assert!(hit, "through point {k}");
                    }
                    // C1 at the joints.
                    for w in spans.windows(2) {
                        let out = (w[0][3] - w[0][2]).normalize();
                        let inn = (w[1][1] - w[1][0]).normalize();
                        assert!(out.dot(&inn) > 1.0 - 1e-9);
                    }
                } else {
                    // Tangent to the first leg.
                    let d = (spans[0][1] - spans[0][0]).normalize();
                    let leg = (Point3::from(pts[1]) - Point3::from(pts[0])).normalize();
                    assert!(d.dot(&leg) > 1.0 - 1e-9, "{kind:?} {n}");
                }
            }
        }
        // Line, spline, line in one chain (the spline added last).
        let mut s = Sketch3d::new("Sketch3D1", "3D Sketch1");
        let a = s.add_point(Point3::new(-50.0, 0.0, 0.0), false);
        let ids: Vec<u32> = pts
            .iter()
            .map(|q| s.add_point(Point3::from(*q), false))
            .collect();
        let z = s.add_point(Point3::new(100.0, 30.0, 100.0), false);
        s.add_line(a, ids[0]).unwrap();
        s.add_line(ids[4], z).unwrap();
        s.add_spline(ids, SplineKind::Interpolation).unwrap();
        let p = s.pieces().unwrap();
        assert_eq!(p.len(), 3);
        assert!(matches!(p[1], PathPiece::Spline { .. }));
        // Each piece starts where the one before ends.
        let pts = |q: &PathPiece| points_of(std::slice::from_ref(q));
        for w in p.windows(2) {
            let (e, s0) = (*pts(&w[0]).last().unwrap(), pts(&w[1])[0]);
            assert!((e - s0).norm() < 1e-9, "joined: {e} {s0}");
        }
    }

    /// A tube swept along a line and a tangent arc, and along a line and a
    /// control-vertex spline starting along it: the volume is π r² × the
    /// path length. (An interpolation spline's end tangent leans towards its
    /// next points: after a line it makes a corner the kernel refuses.)
    #[test]
    fn sweep_along_3d_arcs_and_splines() {
        use crate::part::{
            OriginPlane, PartDocument, PathRef, ProfileRef, SketchOptions, SketchPlacement,
            SweepFeature,
        };
        let r: Real = 3.0;
        for case in [0, 2] {
            let mut s3 = Sketch3d::new("Sketch3D1", "3D Sketch1");
            let o = s3.add_point(Point3::origin(), false);
            let a = s3.add_point(Point3::new(40.0, 0.0, 0.0), false);
            s3.add_line(o, a).unwrap();
            match case {
                0 => {
                    // A quarter arc of r 30 up into Z, tangent to the line.
                    let v = s3.add_point(
                        Point3::new(
                            40.0 + 30.0 * (PI / 4.0).sin(),
                            0.0,
                            30.0 - 30.0 * (PI / 4.0).cos(),
                        ),
                        false,
                    );
                    let b = s3.add_point(Point3::new(70.0, 0.0, 30.0), false);
                    s3.add_arc(a, v, b).unwrap();
                }
                1 => {
                    let ids = vec![
                        a,
                        s3.add_point(Point3::new(70.0, 10.0, 10.0), false),
                        s3.add_point(Point3::new(90.0, 30.0, 30.0), false),
                        s3.add_point(Point3::new(100.0, 60.0, 40.0), false),
                    ];
                    s3.add_spline(ids, SplineKind::Interpolation).unwrap();
                }
                _ => {
                    let ids = vec![
                        a,
                        s3.add_point(Point3::new(70.0, 0.0, 0.0), false),
                        s3.add_point(Point3::new(90.0, 20.0, 20.0), false),
                        s3.add_point(Point3::new(90.0, 50.0, 50.0), false),
                    ];
                    s3.add_spline(ids, SplineKind::ControlVertex).unwrap();
                }
            }
            let len = length_of(&s3.pieces().unwrap());
            let mut part = PartDocument::default();
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
            let want = PI * r * r * len;
            assert!(
                (vol - want).abs() < 5e-3 * want,
                "case {case}: {vol} vs {want}"
            );
        }
    }

    /// Helical curves: the end lands where the helix ends (radius, pitch,
    /// turns, sense), the length is turns × √((2πr)² + p²); a line from
    /// the end chains with it.
    #[test]
    fn helical_curves() {
        for (r, p, turns, cw) in [
            (10.0, 5.0, 1.0, false),
            (10.0, 5.0, 2.5, true),
            (3.0, 1.0, 0.25, false),
            (25.0, 12.0, 4.0, false),
            (6.0, 2.0, 1.75, true),
        ] {
            let mut s = Sketch3d::new("Sketch3D1", "3D Sketch1");
            let c = s.add_point(Point3::new(1.0, 2.0, 3.0), true);
            let st = s.add_point(Point3::new(1.0 + r, 2.0, 3.0), false);
            let h = s.add_helix(c, Vector3::z(), st, p, turns, cw).unwrap();
            let hx = s.helices.iter().find(|x| x.id == h).unwrap().clone();
            let t = std::f64::consts::TAU * turns;
            let sense = if cw { -1.0 } else { 1.0 };
            let want = Point3::new(
                1.0 + r * t.cos(),
                2.0 + sense * r * t.sin(),
                3.0 + p * turns,
            );
            assert!((s.at(hx.end).unwrap() - want).norm() < 1e-9);
            let pieces = s.pieces().unwrap();
            let len = length_of(&pieces);
            let exact = turns * ((std::f64::consts::TAU * r).powi(2) + p * p).sqrt();
            assert!((len - exact).abs() < 2e-3 * exact, "{len} vs {exact}");
            // The Bézier spans stay on the helix (radius within 0.05 %).
            for q in points_of(&pieces) {
                let d = ((q.x - 1.0).powi(2) + (q.y - 2.0).powi(2)).sqrt();
                assert!((d - r).abs() < 5e-4 * r, "{d}");
            }
            let z = s.add_point(want + Vector3::new(0.0, 0.0, 20.0), false);
            s.add_line(hx.end, z).unwrap();
            assert_eq!(s.pieces().unwrap().len(), 2);
        }
        let mut s = Sketch3d::new("Sketch3D1", "3D Sketch1");
        let c = s.add_point(Point3::origin(), true);
        assert!(s.add_helix(c, Vector3::z(), c, 1.0, 1.0, false).is_err());
    }

    /// Dimensions and axis constraints: Length, Distance, Angle, Equal and
    /// Parallel to an axis are met by moving the free points.
    #[test]
    fn dimensions_and_axis_constraints() {
        let mut s = route(&[[0.0, 0.0, 0.0], [40.0, 3.0, 2.0], [55.0, 30.0, 10.0]]);
        s.points[0].fixed = true;
        let (l1, l2) = (s.lines[0].id, s.lines[1].id);
        s.add_constraint(Constraint3::AlongAxis(l1, 0)).unwrap();
        s.add_constraint(Constraint3::Length(l1, 50.0)).unwrap();
        s.add_constraint(Constraint3::Angle(l1, l2, PI / 3.0))
            .unwrap();
        s.add_constraint(Constraint3::Equal(l1, l2)).unwrap();
        let pts: Vec<Point3> = s.points.iter().map(|p| Point3::from(p.at)).collect();
        let d1 = pts[1] - pts[0];
        let d2 = pts[2] - pts[1];
        assert!((d1.norm() - 50.0).abs() < 1e-6, "{}", d1.norm());
        assert!(d1.cross(&Vector3::x()).norm() < 1e-6);
        assert!((d2.norm() - 50.0).abs() < 1e-6);
        let ang = d1.cross(&d2).norm().atan2(d1.dot(&d2));
        assert!((ang - PI / 3.0).abs() < 1e-6, "{}", ang.to_degrees());
        // A distance between the first and last points.
        let (a, c) = (s.points[0].id, s.points[2].id);
        // (The triangle's third side is already 50·√3: a consistent value.)
        let third = 50.0 * 3f64.sqrt();
        s.add_constraint(Constraint3::Distance(a, c, third))
            .unwrap();
        let q = s.at(c).unwrap() - s.at(a).unwrap();
        assert!((q.norm() - third).abs() < 1e-6);
        // Another value conflicts: refused, unchanged.
        let before = s.clone();
        assert!(s.add_constraint(Constraint3::Distance(a, c, 70.0)).is_err());
        assert_eq!(s, before);
        // A negative dimension is refused, the sketch unchanged.
        let before = s.clone();
        assert!(s.add_constraint(Constraint3::Length(l1, -1.0)).is_err());
        assert_eq!(s, before);
    }

    /// Include Geometry: a straight edge is a line, a circular one an arc,
    /// any other a spline; their points are fixed.
    #[test]
    fn included_edges() {
        let mut s = Sketch3d::new("Sketch3D1", "3D Sketch1");
        let k = s
            .include(&[Point3::origin(), Point3::new(10.0, 0.0, 0.0)], false)
            .unwrap();
        assert!(matches!(k, SegId::Line(_)));
        let h = std::f64::consts::FRAC_1_SQRT_2 * 10.0;
        let k = s
            .include(
                &[
                    Point3::new(10.0, 0.0, 0.0),
                    Point3::new(h, h, 0.0),
                    Point3::new(0.0, 10.0, 0.0),
                ],
                true,
            )
            .unwrap();
        assert!(matches!(k, SegId::Arc(_)));
        assert!(s.points.iter().all(|p| p.fixed));
        let p = s.pieces().unwrap();
        assert!((length_of(&p) - (10.0 + PI * 5.0)).abs() < 1e-2);
        let k = s
            .include(
                &[
                    Point3::new(0.0, 10.0, 0.0),
                    Point3::new(-3.0, 14.0, 2.0),
                    Point3::new(-5.0, 20.0, 1.0),
                    Point3::new(-4.0, 25.0, 0.0),
                ],
                false,
            )
            .unwrap();
        assert!(matches!(k, SegId::Spline(_)));
        assert_eq!(s.pieces().unwrap().len(), 3);
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

    #[test]
    fn mirror_copies_lines_and_bends() {
        let mut s = Sketch3d::new("S", "S");
        let a = s.add_point(Point3::new(0.0, 0.0, 0.0), false);
        let b = s.add_point(Point3::new(20.0, 0.0, 0.0), false);
        let c = s.add_point(Point3::new(20.0, 20.0, 5.0), false);
        s.add_line(a, b).unwrap();
        s.add_line(b, c).unwrap();
        s.add_bend(b, 4.0).unwrap();
        let len = s.length();
        // About the plane x = 0: a lies on it and is shared.
        let new = s
            .mirror(&[a, b, c], Point3::origin(), Vector3::x())
            .unwrap();
        assert_eq!(new[0], a);
        assert_eq!(s.lines.len(), 4);
        assert_eq!(s.bends.len(), 2);
        assert_eq!(s.at(new[2]).unwrap(), Point3::new(-20.0, 20.0, 5.0));
        // One chain through a, twice as long.
        let chains = s.chains().unwrap();
        assert_eq!(chains.len(), 1);
        assert!(
            (s.length() - 2.0 * len).abs() < 1e-6,
            "{} vs {len}",
            s.length()
        );
        assert!(s.mirror(&[], Point3::origin(), Vector3::zeros()).is_err());
        // Off the plane: a second chain with its own bend, drawn with it.
        let mut t = Sketch3d::new("T", "T");
        let a = t.add_point(Point3::new(5.0, 0.0, 0.0), false);
        let b = t.add_point(Point3::new(20.0, 0.0, 0.0), false);
        let c = t.add_point(Point3::new(20.0, 20.0, 0.0), false);
        t.add_line(a, b).unwrap();
        t.add_line(b, c).unwrap();
        t.add_bend(b, 4.0).unwrap();
        t.mirror(&[a, b, c], Point3::new(0.0, 0.0, 10.0), Vector3::z())
            .unwrap();
        assert_eq!(t.chains().unwrap().len(), 2);
        assert_eq!(t.bends.len(), 2);
        assert_eq!(t.polylines().len(), 2);
    }
}
