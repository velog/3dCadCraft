//! Corner tools of the Sketch tab (Inventor: Sketch → Create → Fillet /
//! Chamfer) between two lines.
//!
//! Behaviour follows Inventor:
//!
//! - **Fillet** trims both lines back from their (virtual) intersection and
//!   joins them with a tangent arc. The first fillet of a command gets a
//!   radius dimension, every further one an Equal to the first, so one
//!   value drives them all.
//! - **Chamfer** trims both lines and joins them with a straight line, by
//!   two equal distances, two distances, or a distance and an angle. The
//!   virtual sharp corner stays as a construction point with construction
//!   legs to the chamfer ends, so the chamfer stays dimensioned to it.
//!
//! The sides kept are those of the clicks (`hints`); without hints, the
//! longer side of each line. Failures (parallel lines, a size larger than
//! the lines) leave the sketch unchanged.

use serde::{Deserialize, Serialize};

use super::modify::End;
use super::{Constraint, EntityId, Point2, PointId, Shape, Sketch, SketchError, Style, Vector2};
use crate::kernel::Real;

/// How a chamfer is sized (Inventor's 2D Chamfer dialog).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum ChamferKind {
    /// The same distance from the corner along both lines.
    Equal {
        /// Distance from the corner, mm.
        distance: Real,
    },
    /// A distance along each line (first line, second line).
    TwoDistances {
        /// Along the first line, mm.
        first: Real,
        /// Along the second line, mm.
        second: Real,
    },
    /// A distance along the first line and the angle between the first line
    /// and the chamfer.
    DistanceAngle {
        /// Along the first line, mm.
        distance: Real,
        /// Angle between the first line and the chamfer, radians.
        angle: Real,
    },
}

/// One line at a corner: which end gets trimmed and where the kept part lies.
struct Side {
    line: EntityId,
    end: End,
    old: PointId,
    /// Unit direction from the corner towards the kept end.
    u: Vector2,
    /// Distance from the corner to the kept end.
    room: Real,
}

impl Sketch {
    /// The (virtual) corner of two lines and how each is to be trimmed.
    fn corner(
        &self,
        a: EntityId,
        b: EntityId,
        hints: Option<(Point2, Point2)>,
    ) -> Result<(Point2, [Side; 2]), SketchError> {
        if a == b {
            return Err(SketchError::InvalidValue("select two different lines"));
        }
        let not_line = SketchError::InvalidValue("fillet and chamfer work between two lines");
        let (a0, a1) = self.line_geometry(a).ok_or(not_line.clone())?;
        let (b0, b1) = self.line_geometry(b).ok_or(not_line)?;
        let (r, s) = (a1 - a0, b1 - b0);
        let den = r.perp(&s);
        if den.abs() <= 1e-9 * r.norm() * s.norm() {
            return Err(SketchError::InvalidValue("the lines are parallel"));
        }
        let x = a0 + r * ((b0 - a0).perp(&s) / den);
        let side = |line: EntityId, hint: Option<Point2>| -> Result<Side, SketchError> {
            let (p0, p1) = self.line_geometry(line).ok_or(SketchError::UnknownEntity)?;
            let (id0, id1) = self.line_points(line)?;
            let d = (p1 - p0).normalize();
            let (t0, t1, tx) = (0.0, (p1 - p0).norm(), (x - p0).dot(&d));
            // Keep the side of the click, else the longer side.
            let th = match hint {
                Some(h) => (h - p0).dot(&d),
                None if (t0 - tx).abs() > (t1 - tx).abs() => t0,
                None => t1,
            };
            let sign = if th >= tx { 1.0 } else { -1.0 };
            let (kept, room) = if (t0 - tx) * sign >= (t1 - tx) * sign {
                (End::Start, (t0 - tx) * sign)
            } else {
                (End::End, (t1 - tx) * sign)
            };
            if room <= 1e-9 {
                return Err(SketchError::InvalidValue(
                    "the line does not reach the clicked side of the corner",
                ));
            }
            let (end, old) = match kept {
                End::Start => (End::End, id1),
                End::End => (End::Start, id0),
            };
            Ok(Side {
                line,
                end,
                old,
                u: d * sign,
                room,
            })
        };
        let (ha, hb) = hints.map_or((None, None), |(p, q)| (Some(p), Some(q)));
        Ok((x, [side(a, ha)?, side(b, hb)?]))
    }

    /// Moves the trimmed ends of the corner lines to `p` and `q`. An old
    /// end that no other geometry uses is removed — unless constraints
    /// (dimensions, Fix…) refer to it: then it stays as the *virtual sharp*,
    /// held on the line(s) it lay on, so those constraints survive. Returns
    /// the kept point that sits at the corner `x`, if any.
    fn retarget(
        &mut self,
        sides: &[Side; 2],
        p: PointId,
        q: PointId,
        x: Point2,
    ) -> Result<Option<PointId>, SketchError> {
        self.set_end(sides[0].line, sides[0].end, p)?;
        self.set_end(sides[1].line, sides[1].end, q)?;
        let (o1, o2) = (sides[0].old, sides[1].old);
        if o1 != o2 {
            // A Coincident joining the two old ends only described the corner.
            let join: Vec<_> = self
                .constraints()
                .filter(|(_, c)| {
                    matches!(**c, Constraint::Coincident { a, b }
                        if (a == o1 && b == o2) || (a == o2 && b == o1))
                })
                .map(|(id, _)| id)
                .collect();
            for id in join {
                self.remove_constraint(id)?;
            }
        }
        let mut sharp = None;
        for (i, o) in [o1, o2].into_iter().enumerate() {
            if (i == 1 && o == o1) || self.entities().any(|(_, e)| e.points().contains(&o)) {
                continue;
            }
            let Some(at) = self.point(o).map(|q| q.position) else {
                continue;
            };
            let fixed = self.point(o).is_some_and(|q| q.fixed);
            if !fixed && !self.constraints().any(|(_, c)| c.references_point(o)) {
                self.remove_point(o)?;
                continue;
            }
            let at_corner = (at - x).norm() <= 1e-9 * (1.0 + x.coords.norm());
            for (j, side) in sides.iter().enumerate() {
                if j == i || at_corner {
                    self.add_constraint(Constraint::PointOnLine {
                        point: o,
                        line: side.line,
                    })?;
                }
            }
            if at_corner {
                sharp = Some(o);
            }
        }
        Ok(sharp)
    }

    /// **Fillet** lines `a` and `b` with an arc of `radius`. With
    /// `equal_to`, the arc gets an Equal to that earlier fillet arc instead
    /// of its own radius dimension. Returns the new points, the arc and the
    /// constraints.
    pub fn fillet(
        &mut self,
        a: EntityId,
        b: EntityId,
        radius: Real,
        hints: Option<(Point2, Point2)>,
        equal_to: Option<EntityId>,
    ) -> Result<Shape, SketchError> {
        if !(radius.is_finite() && radius > 0.0) {
            return Err(SketchError::InvalidValue("the radius must be positive"));
        }
        let (x, sides) = self.corner(a, b, hints)?;
        let (u1, u2) = (sides[0].u, sides[1].u);
        let half = u1.dot(&u2).clamp(-1.0, 1.0).acos() / 2.0;
        if !(1e-6..=std::f64::consts::FRAC_PI_2 - 1e-6).contains(&half) {
            return Err(SketchError::InvalidValue("the lines are collinear"));
        }
        let t = radius / half.tan();
        if t >= sides[0].room.min(sides[1].room) {
            return Err(SketchError::InvalidValue(
                "the radius is too large for these lines",
            ));
        }
        let (t1, t2) = (x + u1 * t, x + u2 * t);
        let c = x + (u1 + u2).normalize() * (radius / half.sin());
        let mut sh = Shape::default();
        let (p1, p2, pc) = (self.add_point(t1), self.add_point(t2), self.add_point(c));
        sh.points.extend([p1, p2, pc]);
        self.retarget(&sides, p1, p2, x)?;
        // Arcs run counter-clockwise: take the short way between the ends.
        let arc = if (t1 - c).perp(&(t2 - c)) > 0.0 {
            self.add_arc(pc, p1, p2)?
        } else {
            self.add_arc(pc, p2, p1)?
        };
        sh.entities.push(arc);
        let size = match equal_to {
            Some(first) if first != arc => Constraint::Equal { a: first, b: arc },
            _ => Constraint::Radius {
                circle: arc,
                value: radius,
            },
        };
        for c in [
            Constraint::Tangent {
                a,
                b: arc,
                internal: false,
            },
            Constraint::Tangent {
                a: b,
                b: arc,
                internal: false,
            },
            size,
        ] {
            sh.constraints.push(self.add_constraint(c)?);
        }
        Ok(sh)
    }

    /// **Chamfer** lines `a` (first) and `b` (second). Returns the new
    /// points (chamfer ends, then the virtual corner), the chamfer line and
    /// its two construction legs, and the constraints. With `equal_to` (the
    /// first leg of an earlier equal-distance chamfer of the same command)
    /// an equal-distance chamfer gets Equal to it instead of its own
    /// dimension, as Inventor does.
    pub fn chamfer(
        &mut self,
        a: EntityId,
        b: EntityId,
        kind: ChamferKind,
        hints: Option<(Point2, Point2)>,
        equal_to: Option<EntityId>,
    ) -> Result<Shape, SketchError> {
        let (x, sides) = self.corner(a, b, hints)?;
        let (u1, u2) = (sides[0].u, sides[1].u);
        let corner = u1.dot(&u2).clamp(-1.0, 1.0).acos();
        let positive = |v: Real| v.is_finite() && v > 0.0;
        let (d1, d2) = match kind {
            ChamferKind::Equal { distance } => (distance, distance),
            ChamferKind::TwoDistances { first, second } => (first, second),
            ChamferKind::DistanceAngle { distance, angle } => {
                // Triangle corner–T1–T2: angles `corner` at the corner and
                // `angle` at T1, so by the law of sines:
                let third = std::f64::consts::PI - corner - angle;
                if !(positive(angle) && third > 1e-6) {
                    return Err(SketchError::InvalidValue(
                        "the angle does not fit this corner",
                    ));
                }
                (distance, distance * angle.sin() / third.sin())
            }
        };
        if !(positive(d1) && positive(d2)) {
            return Err(SketchError::InvalidValue(
                "chamfer distances must be positive",
            ));
        }
        if d1 >= sides[0].room || d2 >= sides[1].room {
            return Err(SketchError::InvalidValue(
                "the chamfer is too large for these lines",
            ));
        }
        let mut sh = Shape::default();
        let (t1, t2) = (x + u1 * d1, x + u2 * d2);
        let (p1, p2) = (self.add_point(t1), self.add_point(t2));
        // The virtual sharp: the old corner if it is kept, else a new point.
        let (px, kept) = match self.retarget(&sides, p1, p2, x)? {
            Some(o) => (o, true),
            None => (self.add_point(x), false),
        };
        sh.points.extend([p1, p2, px]);
        let line = self.add_line_between(p1, p2)?;
        // Legs run from the chamfer ends to the corner, so the angle to the
        // chamfer is measured at T1 (the value the user typed).
        let leg1 = self.add_line_between(p1, px)?;
        let leg2 = self.add_line_between(p2, px)?;
        for l in [leg1, leg2] {
            self.set_style(l, Style::Construction)?;
        }
        sh.entities.extend([line, leg1, leg2]);
        let mut cs = Vec::new();
        if !kept {
            cs.push(Constraint::PointOnLine { point: px, line: a });
            cs.push(Constraint::PointOnLine { point: px, line: b });
        }
        match (kind, equal_to) {
            (ChamferKind::Equal { .. }, Some(first)) if first != leg1 => {
                cs.push(Constraint::Equal { a: first, b: leg1 })
            }
            _ => cs.push(Constraint::Distance {
                a: px,
                b: p1,
                value: d1,
            }),
        }
        match kind {
            ChamferKind::Equal { .. } => cs.push(Constraint::Equal { a: leg1, b: leg2 }),
            ChamferKind::TwoDistances { .. } => cs.push(Constraint::Distance {
                a: px,
                b: p2,
                value: d2,
            }),
            ChamferKind::DistanceAngle { .. } => {
                let (da, db) = (x - t1, t2 - t1);
                cs.push(Constraint::Angle {
                    a: leg1,
                    b: line,
                    value: da.perp(&db).atan2(da.dot(&db)),
                    supplement: false,
                });
            }
        }
        for c in cs {
            sh.constraints.push(self.add_constraint(c)?);
        }
        Ok(sh)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sketch::{Entity, SolveStatus};
    use approx::assert_relative_eq;

    fn p(x: Real, y: Real) -> Point2 {
        Point2::new(x, y)
    }

    /// 40 × 20 rectangle (H/V, 4 DOF), its corner points and sides.
    fn rect() -> (Sketch, Shape) {
        let mut s = Sketch::default();
        let sh = s
            .add_rectangle_two_point(p(0.0, 0.0), p(40.0, 20.0))
            .unwrap();
        (s, sh)
    }

    fn check(s: &mut Sketch, dof: usize) {
        let r = s.solve().unwrap();
        assert_eq!(
            (r.status, r.dof, r.redundant),
            (SolveStatus::Solved, dof, 0),
            "{r:?}"
        );
    }

    /// The two sides of the rectangle meeting at corner `i`.
    fn sides_at(s: &Sketch, sh: &Shape, i: usize) -> (EntityId, EntityId) {
        let corner = sh.points[i];
        let mut it = sh
            .entities
            .iter()
            .copied()
            .filter(|&e| s.entity(e).unwrap().points().contains(&corner));
        (it.next().unwrap(), it.next().unwrap())
    }

    #[test]
    fn fillet_keeps_the_freedom_and_is_tangent() {
        let (mut s, sh) = rect();
        check(&mut s, 4);
        let (a, b) = sides_at(&s, &sh, 0); // corner (0, 0)
        let f = s.fillet(a, b, 5.0, None, None).unwrap();
        check(&mut s, 4);
        let arc = f.entities[0];
        let (c, r) = s.circle_geometry(arc).unwrap();
        assert_relative_eq!(r, 5.0, epsilon = 1e-9);
        assert_relative_eq!(c, p(5.0, 5.0), epsilon = 1e-9);
        assert!(s.point(sh.points[0]).is_none(), "the sharp corner is gone");
        // The other three corners, equal to the first: still 4 DOF.
        for i in 1..4 {
            let (a, b) = sides_at(&s, &sh, i);
            s.fillet(a, b, 5.0, None, Some(arc)).unwrap();
        }
        check(&mut s, 4);
        let id = s
            .constraints()
            .find(|(_, c)| matches!(c, Constraint::Radius { .. }))
            .unwrap()
            .0;
        s.set_dimension(id, 8.0).unwrap();
        check(&mut s, 4);
        let arcs: Vec<_> = s
            .entities()
            .filter(|(_, e)| matches!(e, Entity::Arc { .. }))
            .map(|(id, _)| id)
            .collect();
        assert_eq!(arcs.len(), 4);
        for a in arcs {
            assert_relative_eq!(s.circle_geometry(a).unwrap().1, 8.0, epsilon = 1e-8);
        }
    }

    #[test]
    fn equal_chamfers_follow_the_first() {
        let (mut s, sh) = rect();
        let (a, b) = sides_at(&s, &sh, 0);
        let first = s
            .chamfer(a, b, ChamferKind::Equal { distance: 2.0 }, None, None)
            .unwrap();
        let leg = first.entities[1];
        let (a, b) = sides_at(&s, &sh, 2);
        s.chamfer(a, b, ChamferKind::Equal { distance: 2.0 }, None, Some(leg))
            .unwrap();
        check(&mut s, 4);
        let d = s
            .constraints()
            .filter(|(_, c)| matches!(c, Constraint::Distance { .. }))
            .map(|(id, _)| id)
            .collect::<Vec<_>>();
        assert_eq!(d.len(), 1, "one dimension drives both");
        s.set_dimension(d[0], 3.0).unwrap();
        check(&mut s, 4);
        let (p, q) = s.line_geometry(leg).unwrap();
        assert!(((p - q).norm() - 3.0).abs() < 1e-9);
    }

    #[test]
    fn dimensions_to_a_filleted_corner_survive() {
        // Fully defined: fixed corner 0, width and height dimensioned to corner 2.
        let (mut s, sh) = rect();
        let pt = &sh.points;
        s.set_fixed(pt[0], true).unwrap();
        for (q, v) in [(pt[1], 40.0), (pt[3], 20.0)] {
            s.add_constraint(Constraint::Distance {
                a: pt[0],
                b: q,
                value: v,
            })
            .unwrap();
        }
        check(&mut s, 0);
        let n = s.constraints().count();
        for i in [0, 2] {
            let (a, b) = sides_at(&s, &sh, i);
            s.fillet(a, b, 3.0, None, None).unwrap();
        }
        let (a, b) = sides_at(&s, &sh, 1);
        s.chamfer(a, b, ChamferKind::Equal { distance: 2.0 }, None, None)
            .unwrap();
        // Corners 0 (fixed) and 1 (dimensioned) stay as virtual sharps.
        assert!(s.point(pt[0]).is_some() && s.point(pt[1]).is_some());
        assert!(s.constraints().count() > n);
        check(&mut s, 0);
    }

    #[test]
    fn fillet_failures_leave_the_sketch_unchanged() {
        let (mut s, sh) = rect();
        let json = |s: &Sketch| serde_json::to_string(s).unwrap();
        let before = json(&s);
        let (a, b) = sides_at(&s, &sh, 0);
        assert!(s.fillet(a, b, 25.0, None, None).is_err(), "too large");
        assert!(s.fillet(a, b, -1.0, None, None).is_err());
        let (top, bottom) = (sh.entities[2], sh.entities[0]);
        assert!(s.fillet(top, bottom, 1.0, None, None).is_err(), "parallel");
        assert_eq!(json(&s), before);
    }

    #[test]
    fn fillet_crossing_lines_keeps_the_clicked_sides() {
        let mut s = Sketch::default();
        let h = s.add_line(p(-10.0, 0.0), p(10.0, 0.0));
        let v = s.add_line(p(0.0, -10.0), p(0.0, 10.0));
        s.fillet(h, v, 2.0, Some((p(6.0, 0.0), p(0.0, 6.0))), None)
            .unwrap();
        // 5 points (10) − arc end on circle − 2 Tangent − Radius.
        check(&mut s, 6);
        let (h0, h1) = s.line_geometry(h).unwrap();
        let (v0, v1) = s.line_geometry(v).unwrap();
        assert_relative_eq!(h0, p(2.0, 0.0), epsilon = 1e-9);
        assert_relative_eq!(h1, p(10.0, 0.0), epsilon = 1e-9);
        assert_relative_eq!(v0, p(0.0, 2.0), epsilon = 1e-9);
        assert_relative_eq!(v1, p(0.0, 10.0), epsilon = 1e-9);
    }

    #[test]
    fn chamfer_kinds_keep_the_freedom() {
        let deg = std::f64::consts::PI / 180.0;
        for kind in [
            ChamferKind::Equal { distance: 4.0 },
            ChamferKind::TwoDistances {
                first: 4.0,
                second: 6.0,
            },
            ChamferKind::DistanceAngle {
                distance: 4.0,
                angle: 30.0 * deg,
            },
        ] {
            let (mut s, sh) = rect();
            let (a, b) = sides_at(&s, &sh, 0);
            let c = s.chamfer(a, b, kind, None, None).unwrap();
            check(&mut s, 4);
            let (t1, t2) = s.line_geometry(c.entities[0]).unwrap();
            let x = s.point(c.points[2]).unwrap().position;
            assert_relative_eq!(x, p(0.0, 0.0), epsilon = 1e-9);
            assert_relative_eq!((t1 - x).norm(), 4.0, epsilon = 1e-9);
            let expect = match kind {
                ChamferKind::Equal { .. } => 4.0,
                ChamferKind::TwoDistances { second, .. } => second,
                ChamferKind::DistanceAngle { .. } => 4.0 * (30.0 * deg).tan(),
            };
            assert_relative_eq!((t2 - x).norm(), expect, epsilon = 1e-9);
        }
    }
}
