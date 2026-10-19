//! Composite sketch shapes, created the way Inventor's Create panel creates
//! them: the geometry plus the constraints (and construction geometry) that
//! keep the shape's character when it is dragged or dimensioned.
//!
//! Every builder is checked by tests for the exact degrees of freedom the
//! shape should have and for **zero redundant constraints** — a redundant
//! constraint would show the fresh shape as over-constrained (orange).
//!
//! | Shape | Input (Inventor click sequence) | Constraints | DOF |
//! |---|---|---|---|
//! | Two Point Rectangle | opposite corners | H, V, H, V | 4 |
//! | Three Point Rectangle | corner, side end, adjacent side | ∥, ∥, ⟂ | 5 |
//! | Two Point Center Rectangle | centre, corner | H, V, H, V, centre = midpoint of a construction diagonal | 4 |
//! | Three Point Center Rectangle | centre, side direction/half-length, adjacent half-width | ∥, ∥, ⟂, midpoint | 5 |
//! | Center to Center Slot | arc centres, width | 4 × tangent, equal arcs, construction centreline | 5 |
//! | Overall Slot | slot ends, width | same | 5 |
//! | Center Point Slot | slot centre, arc centre, width | same + centre = midpoint of centreline | 5 |
//! | Arc slot (3-point / centre point) | centre arc, width | 4 × tangent; radial construction lines | 6 |
//! | Polygon (inscribed) | centre, vertex | vertices on construction circle, equal sides | 4 |
//! | Polygon (circumscribed) | centre, side midpoint | sides tangent to construction circle, equal sides (even N: one fewer, + equal vertex radii — Pitot) | 4 |
//! | Tangent Circle | three lines | 3 × tangent | 0 relative to the lines |

use super::{
    Constraint, ConstraintId, EntityId, Point2, PointId, Sketch, SketchError, Style, Vector2,
};
use crate::kernel::Real;

/// What a builder created.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shape {
    /// Points created.
    pub points: Vec<PointId>,
    /// Entities created (including construction geometry).
    pub entities: Vec<EntityId>,
    /// Constraints created.
    pub constraints: Vec<ConstraintId>,
}

fn perp(v: Vector2) -> Vector2 {
    Vector2::new(-v.y, v.x)
}

fn unit(v: Vector2, what: &'static str) -> Result<Vector2, SketchError> {
    let n = v.norm();
    if n < 1e-9 {
        return Err(SketchError::InvalidValue(what));
    }
    Ok(v / n)
}

impl Shape {
    fn point(&mut self, s: &mut Sketch, p: Point2) -> PointId {
        let id = s.add_point(p);
        self.points.push(id);
        id
    }
    fn line(&mut self, s: &mut Sketch, a: PointId, b: PointId) -> Result<EntityId, SketchError> {
        let id = s.add_line_between(a, b)?;
        self.entities.push(id);
        Ok(id)
    }
    fn construction(
        &mut self,
        s: &mut Sketch,
        a: PointId,
        b: PointId,
    ) -> Result<EntityId, SketchError> {
        let id = self.line(s, a, b)?;
        s.set_style(id, Style::Construction)?;
        Ok(id)
    }
    fn arc(
        &mut self,
        s: &mut Sketch,
        c: PointId,
        a: PointId,
        b: PointId,
    ) -> Result<EntityId, SketchError> {
        let id = s.add_arc(c, a, b)?;
        self.entities.push(id);
        Ok(id)
    }
    fn constrain(&mut self, s: &mut Sketch, c: Constraint) -> Result<(), SketchError> {
        self.constraints.push(s.add_constraint(c)?);
        Ok(())
    }
    fn tangent(&mut self, s: &mut Sketch, a: EntityId, b: EntityId) -> Result<(), SketchError> {
        self.constraints.push(s.add_tangent(a, b)?);
        Ok(())
    }
}

impl Sketch {
    /// Closed polyline through `corners` (4 lines for a rectangle).
    fn closed_loop(
        &mut self,
        shape: &mut Shape,
        corners: &[Point2],
    ) -> Result<(Vec<PointId>, Vec<EntityId>), SketchError> {
        let pts: Vec<PointId> = corners.iter().map(|&c| shape.point(self, c)).collect();
        let mut lines = Vec::new();
        for i in 0..pts.len() {
            lines.push(shape.line(self, pts[i], pts[(i + 1) % pts.len()])?);
        }
        Ok((pts, lines))
    }

    /// **Two Point Rectangle**: axis-aligned, from opposite corners.
    pub fn add_rectangle_two_point(
        &mut self,
        p1: Point2,
        p2: Point2,
    ) -> Result<Shape, SketchError> {
        if (p1.x - p2.x).abs() < 1e-9 || (p1.y - p2.y).abs() < 1e-9 {
            return Err(SketchError::InvalidValue(
                "rectangle has zero width or height",
            ));
        }
        let mut sh = Shape::default();
        let corners = [p1, Point2::new(p2.x, p1.y), p2, Point2::new(p1.x, p2.y)];
        let (_, l) = self.closed_loop(&mut sh, &corners)?;
        for (i, &line) in l.iter().enumerate() {
            let c = if i % 2 == 0 {
                Constraint::Horizontal { line }
            } else {
                Constraint::Vertical { line }
            };
            sh.constrain(self, c)?;
        }
        Ok(sh)
    }

    /// Corners of a rectangle given one side `a → b` and a point `c` that
    /// sets the adjacent side (any orientation).
    fn three_point_corners(a: Point2, b: Point2, c: Point2) -> Result<[Point2; 4], SketchError> {
        let u = unit(b - a, "rectangle side has zero length")?;
        let n = perp(u);
        let h = (c - a).dot(&n);
        if h.abs() < 1e-9 {
            return Err(SketchError::InvalidValue("rectangle has zero height"));
        }
        Ok([a, b, b + n * h, a + n * h])
    }

    /// Parallel/parallel/perpendicular for a free-orientation rectangle.
    fn orient_free(&mut self, sh: &mut Shape, l: &[EntityId]) -> Result<(), SketchError> {
        sh.constrain(self, Constraint::Parallel { a: l[0], b: l[2] })?;
        sh.constrain(self, Constraint::Parallel { a: l[1], b: l[3] })?;
        sh.constrain(self, Constraint::Perpendicular { a: l[0], b: l[1] })
    }

    /// **Three Point Rectangle**: corner, end of the first side, adjacent side.
    pub fn add_rectangle_three_point(
        &mut self,
        p1: Point2,
        p2: Point2,
        p3: Point2,
    ) -> Result<Shape, SketchError> {
        let corners = Self::three_point_corners(p1, p2, p3)?;
        let mut sh = Shape::default();
        let (_, l) = self.closed_loop(&mut sh, &corners)?;
        self.orient_free(&mut sh, &l)?;
        Ok(sh)
    }

    /// Centre point on a construction diagonal (shared by both centre
    /// rectangles). One diagonal suffices: in a rectangle the other passes
    /// through the same point, so a second midpoint would be redundant.
    fn center_on_diagonal(
        &mut self,
        sh: &mut Shape,
        center: Point2,
        pts: &[PointId],
    ) -> Result<(), SketchError> {
        let c = sh.point(self, center);
        let d1 = sh.construction(self, pts[0], pts[2])?;
        sh.construction(self, pts[1], pts[3])?;
        sh.constrain(self, Constraint::Midpoint { point: c, line: d1 })
    }

    /// **Two Point Center Rectangle**: centre and a corner.
    pub fn add_rectangle_center(
        &mut self,
        center: Point2,
        corner: Point2,
    ) -> Result<Shape, SketchError> {
        let d = corner - center;
        if d.x.abs() < 1e-9 || d.y.abs() < 1e-9 {
            return Err(SketchError::InvalidValue(
                "rectangle has zero width or height",
            ));
        }
        let corners = [
            center - d,
            center + Vector2::new(d.x, -d.y),
            center + d,
            center + Vector2::new(-d.x, d.y),
        ];
        let mut sh = Shape::default();
        let (pts, l) = self.closed_loop(&mut sh, &corners)?;
        for (i, &line) in l.iter().enumerate() {
            let c = if i % 2 == 0 {
                Constraint::Horizontal { line }
            } else {
                Constraint::Vertical { line }
            };
            sh.constrain(self, c)?;
        }
        self.center_on_diagonal(&mut sh, center, &pts)?;
        Ok(sh)
    }

    /// **Three Point Center Rectangle**: centre, side direction and half
    /// length, adjacent half width.
    pub fn add_rectangle_center_three_point(
        &mut self,
        center: Point2,
        p2: Point2,
        p3: Point2,
    ) -> Result<Shape, SketchError> {
        let half = p2 - center;
        let u = unit(half, "rectangle side has zero length")?;
        let n = perp(u);
        let w = (p3 - center).dot(&n);
        if w.abs() < 1e-9 {
            return Err(SketchError::InvalidValue("rectangle has zero height"));
        }
        let (a, b) = (half, n * w);
        let corners = [
            center - a - b,
            center + a - b,
            center + a + b,
            center - a + b,
        ];
        let mut sh = Shape::default();
        let (pts, l) = self.closed_loop(&mut sh, &corners)?;
        self.orient_free(&mut sh, &l)?;
        self.center_on_diagonal(&mut sh, center, &pts)?;
        Ok(sh)
    }

    /// **Center to Center Slot**: straight slot between arc centres `c1`,
    /// `c2` with total `width`.
    pub fn add_slot_center_to_center(
        &mut self,
        c1: Point2,
        c2: Point2,
        width: Real,
    ) -> Result<Shape, SketchError> {
        Ok(self.straight_slot(c1, c2, width)?.0)
    }

    /// **Overall Slot**: straight slot whose rounded ends are at `e1`, `e2`.
    pub fn add_slot_overall(
        &mut self,
        e1: Point2,
        e2: Point2,
        width: Real,
    ) -> Result<Shape, SketchError> {
        let u = unit(e2 - e1, "slot has zero length")?;
        let r = width / 2.0;
        if (e2 - e1).norm() <= width {
            return Err(SketchError::InvalidValue(
                "overall slot length must exceed its width",
            ));
        }
        Ok(self.straight_slot(e1 + u * r, e2 - u * r, width)?.0)
    }

    /// **Center Point Slot**: slot centre, one arc centre, width.
    pub fn add_slot_center_point(
        &mut self,
        center: Point2,
        arc_center: Point2,
        width: Real,
    ) -> Result<Shape, SketchError> {
        let other = center - (arc_center - center);
        let (mut sh, axis) = self.straight_slot(other, arc_center, width)?;
        let c = sh.point(self, center);
        sh.constrain(
            self,
            Constraint::Midpoint {
                point: c,
                line: axis,
            },
        )?;
        Ok(sh)
    }

    /// Straight slot; returns the shape and its construction centreline.
    fn straight_slot(
        &mut self,
        c1: Point2,
        c2: Point2,
        width: Real,
    ) -> Result<(Shape, EntityId), SketchError> {
        if !(width.is_finite() && width > 0.0) {
            return Err(SketchError::InvalidValue("slot width must be positive"));
        }
        let u = unit(c2 - c1, "slot centres coincide")?;
        let n = perp(u) * (width / 2.0);
        let mut sh = Shape::default();
        let pc1 = sh.point(self, c1);
        let pc2 = sh.point(self, c2);
        let a_top = sh.point(self, c1 + n);
        let a_bot = sh.point(self, c1 - n);
        let b_top = sh.point(self, c2 + n);
        let b_bot = sh.point(self, c2 - n);
        let left = sh.arc(self, pc1, a_top, a_bot)?; // CCW through the far side of c1
        let right = sh.arc(self, pc2, b_bot, b_top)?;
        let top = sh.line(self, b_top, a_top)?;
        let bottom = sh.line(self, a_bot, b_bot)?;
        let axis = sh.construction(self, pc1, pc2)?;
        for (line, arc) in [(top, left), (top, right), (bottom, left), (bottom, right)] {
            sh.tangent(self, line, arc)?;
        }
        sh.constrain(self, Constraint::Equal { a: left, b: right })?;
        Ok((sh, axis))
    }

    /// **Arc slot**: a slot bent along the arc around `center` from `start`
    /// (counter-clockwise) to the direction of `end`, with total `width`.
    /// Serves both Inventor variants (*Three Point Arc Slot* computes the
    /// centre from three points first, *Center Point Arc Slot* gets it
    /// directly).
    pub fn add_arc_slot(
        &mut self,
        center: Point2,
        start: Point2,
        end: Point2,
        width: Real,
    ) -> Result<Shape, SketchError> {
        let radius = (start - center).norm();
        let r = width / 2.0;
        if !(width.is_finite() && width > 0.0) || r >= radius {
            return Err(SketchError::InvalidValue(
                "slot width must be positive and smaller than the arc diameter",
            ));
        }
        let us = unit(start - center, "slot arc has zero radius")?;
        let ue = unit(end - center, "slot end direction is undefined")?;
        let e = center + ue * radius;
        let mut sh = Shape::default();
        let o = sh.point(self, center);
        let s = sh.point(self, start);
        let pe = sh.point(self, e);
        let s_out = sh.point(self, start + us * r);
        let s_in = sh.point(self, start - us * r);
        let e_out = sh.point(self, e + ue * r);
        let e_in = sh.point(self, e - ue * r);
        let outer = sh.arc(self, o, s_out, e_out)?;
        let inner = sh.arc(self, o, s_in, e_in)?;
        let cap_s = sh.arc(self, s, s_in, s_out)?; // bulges behind the start
        let cap_e = sh.arc(self, pe, e_out, e_in)?; // bulges beyond the end
        // Radial construction lines (for angle dimensions). A construction
        // centre arc would add an equation the tangencies already imply.
        sh.construction(self, o, s)?;
        sh.construction(self, o, pe)?;
        for (cap, side) in [
            (cap_s, outer),
            (cap_s, inner),
            (cap_e, outer),
            (cap_e, inner),
        ] {
            sh.tangent(self, cap, side)?;
        }
        Ok(sh)
    }

    /// **Polygon** with `n` sides around `center`. Inscribed: `corner` is a
    /// vertex on the construction circle. Circumscribed: `corner` is the
    /// midpoint of a side, which touches the construction circle there.
    pub fn add_polygon(
        &mut self,
        center: Point2,
        corner: Point2,
        n: usize,
        inscribed: bool,
    ) -> Result<Shape, SketchError> {
        if !(3..=120).contains(&n) {
            return Err(SketchError::InvalidValue("a polygon has 3 to 120 sides"));
        }
        let d = corner - center;
        let r = d.norm();
        if r < 1e-9 {
            return Err(SketchError::InvalidValue("polygon has zero size"));
        }
        let step = std::f64::consts::TAU / n as Real;
        let a0 = d.y.atan2(d.x);
        // Vertex radius and angle offset: inscribed vertices sit on the
        // circle; circumscribed sides are tangent to it at their midpoints.
        let (vr, off) = if inscribed {
            (r, 0.0)
        } else {
            (r / (step / 2.0).cos(), step / 2.0)
        };
        let verts: Vec<Point2> = (0..n)
            .map(|i| {
                let a = a0 - off + step * i as Real;
                center + Vector2::new(a.cos(), a.sin()) * vr
            })
            .collect();

        let mut sh = Shape::default();
        let c = sh.point(self, center);
        let circle = self.add_circle_at(c, r)?;
        self.set_style(circle, Style::Construction)?;
        sh.entities.push(circle);
        let (pts, lines) = self.closed_loop(&mut sh, &verts)?;
        if inscribed {
            for &p in &pts {
                sh.constrain(self, Constraint::PointOnCircle { point: p, circle })?;
            }
        } else {
            for &l in &lines {
                sh.tangent(self, l, circle)?;
            }
            // Tangent + equal sides still allow alternating corners (a
            // rhombus) when n is even; equal vertex radii remove that.
            if n.is_multiple_of(2) {
                let r0 = sh.construction(self, c, pts[0])?;
                let r1 = sh.construction(self, c, pts[1])?;
                sh.constrain(self, Constraint::Equal { a: r0, b: r1 })?;
            }
        }
        // For a circumscribed polygon with even n, Pitot's theorem (the
        // alternating sum of the sides of a tangential polygon is zero) makes
        // the last "equal sides" relation follow from the others; adding it
        // would be redundant.
        let skip_last = !inscribed && n.is_multiple_of(2);
        let equal_to = if skip_last {
            &lines[1..n - 1]
        } else {
            &lines[1..]
        };
        for &l in equal_to {
            sh.constrain(self, Constraint::Equal { a: lines[0], b: l })?;
        }
        Ok(sh)
    }

    /// **Tangent Circle**: a circle tangent to three lines, starting from a
    /// guess (`center`, `radius`) and letting the solver place it.
    pub fn add_tangent_circle(
        &mut self,
        lines: [EntityId; 3],
        center: Point2,
        radius: Real,
    ) -> Result<Shape, SketchError> {
        let mut sh = Shape::default();
        let c = sh.point(self, center);
        let circle = self.add_circle_at(c, radius.max(1e-3))?;
        sh.entities.push(circle);
        for l in lines {
            sh.tangent(self, l, circle)?;
        }
        Ok(sh)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sketch::{SolveReport, SolveStatus};
    use approx::assert_relative_eq;

    fn p(x: Real, y: Real) -> Point2 {
        Point2::new(x, y)
    }

    fn solved(s: &mut Sketch) -> SolveReport {
        let r = s.solve().unwrap();
        assert_eq!(r.status, SolveStatus::Solved, "{r:?}");
        r
    }

    fn assert_dof(s: &mut Sketch, dof: usize) {
        let r = solved(s);
        assert_eq!((r.dof, r.redundant), (dof, 0), "{r:?}");
    }

    #[test]
    fn rectangles_have_inventor_dof() {
        let mut s = Sketch::default();
        s.add_rectangle_two_point(p(0.0, 0.0), p(30.0, 20.0))
            .unwrap();
        assert_dof(&mut s, 4);

        let mut s = Sketch::default();
        s.add_rectangle_three_point(p(0.0, 0.0), p(30.0, 10.0), p(5.0, 25.0))
            .unwrap();
        assert_dof(&mut s, 5);

        let mut s = Sketch::default();
        let sh = s
            .add_rectangle_center(p(10.0, 10.0), p(25.0, 18.0))
            .unwrap();
        assert_dof(&mut s, 4);
        assert_eq!(s.style(sh.entities[4]), Style::Construction);

        let mut s = Sketch::default();
        s.add_rectangle_center_three_point(p(0.0, 0.0), p(20.0, 10.0), p(-3.0, 9.0))
            .unwrap();
        assert_dof(&mut s, 5);
    }

    #[test]
    fn slots_have_inventor_dof() {
        let mut s = Sketch::default();
        s.add_slot_center_to_center(p(0.0, 0.0), p(30.0, 0.0), 10.0)
            .unwrap();
        assert_dof(&mut s, 5);

        let mut s = Sketch::default();
        let sh = s.add_slot_overall(p(0.0, 0.0), p(40.0, 0.0), 10.0).unwrap();
        assert_dof(&mut s, 5);
        // The first arc centre sits half a width in from the overall end.
        assert_relative_eq!(
            s.point(sh.points[0]).unwrap().position,
            p(5.0, 0.0),
            epsilon = 1e-9
        );

        let mut s = Sketch::default();
        s.add_slot_center_point(p(0.0, 0.0), p(15.0, 5.0), 8.0)
            .unwrap();
        assert_dof(&mut s, 5);

        let mut s = Sketch::default();
        s.add_arc_slot(p(0.0, 0.0), p(30.0, 0.0), p(0.0, 30.0), 6.0)
            .unwrap();
        assert_dof(&mut s, 6);
    }

    #[test]
    fn polygons_are_regular_with_four_dof() {
        for n in [3, 4, 5, 6] {
            for inscribed in [true, false] {
                let mut s = Sketch::default();
                let sh = s
                    .add_polygon(p(0.0, 0.0), p(10.0, 0.0), n, inscribed)
                    .unwrap();
                let r = solved(&mut s);
                assert_eq!(
                    (r.dof, r.redundant),
                    (4, 0),
                    "n={n} inscribed={inscribed}: {r:?}"
                );
                // Regular: all vertices equally far from the centre.
                let c = s.point(sh.points[0]).unwrap().position;
                let radii: Vec<Real> = sh.points[1..]
                    .iter()
                    .map(|&q| (s.point(q).unwrap().position - c).norm())
                    .collect();
                for r in &radii {
                    assert_relative_eq!(*r, radii[0], epsilon = 1e-9);
                }
            }
        }
        assert!(
            Sketch::default()
                .add_polygon(p(0.0, 0.0), p(1.0, 0.0), 2, true)
                .is_err()
        );
    }

    #[test]
    fn tangent_circle_finds_the_incircle() {
        // Triangle (0,0)-(40,0)-(0,30): incircle radius (a+b−c)/2 = 10, centre (10,10).
        let mut s = Sketch::default();
        let a = s.add_fixed_point(p(0.0, 0.0));
        let b = s.add_fixed_point(p(40.0, 0.0));
        let c = s.add_fixed_point(p(0.0, 30.0));
        let l = [
            s.add_line_between(a, b).unwrap(),
            s.add_line_between(b, c).unwrap(),
            s.add_line_between(c, a).unwrap(),
        ];
        let sh = s.add_tangent_circle(l, p(12.0, 8.0), 6.0).unwrap();
        assert_dof(&mut s, 0);
        let (centre, r) = s.circle_geometry(sh.entities[0]).unwrap();
        assert_relative_eq!(r, 10.0, epsilon = 1e-7);
        assert_relative_eq!(centre, p(10.0, 10.0), epsilon = 1e-7);
    }
}
