//! Sketch constraints: geometric relations and dimensions, their residual
//! functions (zero ⇔ satisfied) and their measured values.
//!
//! The set follows Inventor's 2D sketch constraints (Sketch → Constrain):
//! Coincident (point–point and point-on-curve), Collinear, Concentric, Fix,
//! Parallel, Perpendicular, Horizontal/Vertical (line or two points),
//! Tangent, Symmetric, Equal — plus the dimension kinds the General
//! Dimension tool creates (aligned/horizontal/vertical distance,
//! point–line distance, angle, radius, diameter). Smooth (G2) needs spline
//! curvature and is not implemented yet.

use serde::{Deserialize, Serialize};

use super::{Entity, EntityId, Point2, PointId, Sketch, SketchError, Vector2};
use crate::kernel::Real;

/// A geometric or dimensional relation the solver must satisfy.
///
/// Serialised with a `"type"` tag, e.g.
/// `{"type":"Parallel","a":"1v1","b":"2v1"}`, which is the form commands and
/// external agents use.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Constraint {
    /// Two points occupy the same position.
    Coincident {
        /// First point.
        a: PointId,
        /// Second point.
        b: PointId,
    },
    /// A point lies on the infinite extension of a line (Inventor:
    /// Coincident point–curve; the point may slide along).
    PointOnLine {
        /// The point.
        point: PointId,
        /// A line entity.
        line: EntityId,
    },
    /// A point lies on a circle or on an arc's circle.
    PointOnCircle {
        /// The point.
        point: PointId,
        /// A circle or arc entity.
        circle: EntityId,
    },
    /// A point lies on a spline (Inventor: Coincident point–spline). The
    /// point may slide along it; past the ends it follows the end tangents.
    PointOnSpline {
        /// The point.
        point: PointId,
        /// A spline entity.
        spline: EntityId,
    },
    /// Pattern copy, translation: `a2 − a = b2 − b` (the copy moved like
    /// the first step `b → b2` of a rectangular pattern).
    ///
    /// With `arc_center` (the centre of an arc ending at `a2`) only the
    /// component tangent to that arc is kept: the arc's own "end on the
    /// circle" equation supplies the radial one.
    Translated {
        /// Point of the previous copy.
        a: PointId,
        /// The same point in this copy.
        a2: PointId,
        /// Anchor of the pattern.
        b: PointId,
        /// The anchor in the first step.
        b2: PointId,
        /// Centre of the copied arc ending at `a2`, see above.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        arc_center: Option<PointId>,
        /// Extra displacement (mm, sketch x/y): `a2 − a = b2 − b + offset`
        /// (an individually offset pattern element and the one after it).
        #[serde(default, skip_serializing_if = "is_zero2")]
        offset: [Real; 2],
    },
    /// Pattern copy, rotation: `a2 − c = R·(a − c)`, where `R` is the
    /// rotation about `center` that takes `b` to `b2` (the first step of a
    /// circular pattern). `arc_center` as for [`Constraint::Translated`].
    Rotated {
        /// Centre of the pattern.
        center: PointId,
        /// Point of the previous copy.
        a: PointId,
        /// The same point in this copy.
        a2: PointId,
        /// Anchor of the pattern.
        b: PointId,
        /// The anchor in the first step.
        b2: PointId,
        /// Centre of the copied arc ending at `a2`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        arc_center: Option<PointId>,
    },
    /// A point lies at the middle of a line.
    Midpoint {
        /// The point.
        point: PointId,
        /// A line entity.
        line: EntityId,
    },
    /// Two lines lie on one infinite line.
    Collinear {
        /// First line.
        a: EntityId,
        /// Second line.
        b: EntityId,
    },
    /// Two circles/arcs share their centre position.
    Concentric {
        /// First circle or arc.
        a: EntityId,
        /// Second circle or arc.
        b: EntityId,
    },
    /// A point is held at a position (Inventor: Fix). Fixing a curve fixes
    /// its defining points.
    Fix {
        /// The point.
        point: PointId,
        /// Where it is held (sketch coordinates).
        at: Point2,
    },
    /// A line is parallel to the sketch's `u` axis.
    Horizontal {
        /// A line entity.
        line: EntityId,
    },
    /// A line is parallel to the sketch's `v` axis.
    Vertical {
        /// A line entity.
        line: EntityId,
    },
    /// Two points lie on one horizontal line.
    HorizontalPoints {
        /// First point.
        a: PointId,
        /// Second point.
        b: PointId,
    },
    /// Two points lie on one vertical line.
    VerticalPoints {
        /// First point.
        a: PointId,
        /// Second point.
        b: PointId,
    },
    /// Two lines are parallel (or anti-parallel).
    Parallel {
        /// First line.
        a: EntityId,
        /// Second line.
        b: EntityId,
    },
    /// Two lines meet at a right angle.
    Perpendicular {
        /// First line.
        a: EntityId,
        /// Second line.
        b: EntityId,
    },
    /// A line and a circle/arc, or two circles/arcs, touch tangentially.
    ///
    /// `internal` only matters for two circles: `true` means one lies inside
    /// the other (`‖c₁ − c₂‖ = |r₁ − r₂|`), `false` means they touch from
    /// outside (`‖c₁ − c₂‖ = r₁ + r₂`). [`Sketch::add_tangent`] picks it from
    /// the current geometry.
    Tangent {
        /// First entity.
        a: EntityId,
        /// Second entity.
        b: EntityId,
        /// Internal (nested) contact for circle–circle tangency.
        internal: bool,
    },
    /// Two lines have equal length, or two circles/arcs equal radius.
    Equal {
        /// First entity.
        a: EntityId,
        /// Second entity of the same kind.
        b: EntityId,
    },
    /// Points `a` and `b` are mirror images about `line`.
    Symmetric {
        /// First point.
        a: PointId,
        /// Second point.
        b: PointId,
        /// Symmetry line.
        line: EntityId,
    },
    /// [`Constraint::Symmetric`] reduced to one equation: only the component
    /// of `b − mirror(a)` tangent to the arc around `about` that ends at `b`
    /// (the arc's own "end on the circle" equation supplies the rest; Mirror
    /// uses it so mirrored arcs are not over-constrained).
    SymmetricOnArc {
        /// First point.
        a: PointId,
        /// Second point (an end of the mirrored arc).
        b: PointId,
        /// Symmetry line.
        line: EntityId,
        /// Centre of the arc ending at `b` (or a point of it, see Mirror).
        about: PointId,
    },
    /// Two offset pairs keep the same offset (Inventor's *Constrain Offset*):
    /// `|offset(copy_a, base_a)| = |offset(copy_b, base_b)|`, where the offset
    /// of a line is the distance of its start from the base line and of a
    /// circle/arc the radius difference.
    EqualOffset {
        /// Base of the first pair.
        base_a: EntityId,
        /// Offset copy of `base_a`.
        copy_a: EntityId,
        /// Base of the second pair.
        base_b: EntityId,
        /// Offset copy of `base_b`.
        copy_b: EntityId,
    },
    /// Aligned distance between two points (mm, > 0).
    Distance {
        /// First point.
        a: PointId,
        /// Second point.
        b: PointId,
        /// Required distance.
        value: Real,
    },
    /// Horizontal distance `b.x − a.x` (mm, > 0; points ordered so).
    HorizontalDistance {
        /// Left point.
        a: PointId,
        /// Right point.
        b: PointId,
        /// Required distance.
        value: Real,
    },
    /// Vertical distance `b.y − a.y` (mm, > 0; points ordered so).
    VerticalDistance {
        /// Lower point.
        a: PointId,
        /// Upper point.
        b: PointId,
        /// Required distance.
        value: Real,
    },
    /// Perpendicular distance from a point to a line (mm, > 0). Also used
    /// for the distance between two parallel lines.
    PointLineDistance {
        /// The point.
        point: PointId,
        /// The line.
        line: EntityId,
        /// Required distance.
        value: Real,
    },
    /// Diametric dimension to a centerline (Inventor: a dimension placed
    /// on the far side of a centerline): twice the distance from `point`
    /// to the axis `line` — the diameter a revolve gives it.
    DiameterToAxis {
        /// The point (or a point of the measured line).
        point: PointId,
        /// The axis (usually a centerline).
        line: EntityId,
        /// Required diameter.
        value: Real,
    },
    /// Signed angle from line `a` to line `b` (radians, in (−π, π]).
    Angle {
        /// First line.
        a: EntityId,
        /// Second line.
        b: EntityId,
        /// Required angle.
        value: Real,
        /// Measure to `b` run backwards: the supplementary angle (the
        /// quadrant the dimension was placed in, as in Inventor).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        supplement: bool,
    },
    /// A circle's or arc's radius (mm, > 0).
    Radius {
        /// A circle or arc entity.
        circle: EntityId,
        /// Required radius.
        value: Real,
    },
    /// A circle's diameter (mm, > 0).
    Diameter {
        /// A circle or arc entity.
        circle: EntityId,
        /// Required diameter.
        value: Real,
    },
}

/// What kind of entity a constraint argument must be.
#[derive(Clone, Copy)]
enum Kind {
    Line,
    Circular,
    LineOrCircular,
    Spline,
}

/// Read access to positions — either the stored geometry or the solver's
/// candidate values — so residuals and measurements share one formula.
pub(crate) trait Geo {
    fn sketch(&self) -> &Sketch;
    fn point(&self, id: PointId) -> Vector2;
    fn radius_param(&self, id: EntityId, stored: Real) -> Real;

    /// Endpoints of a line.
    fn line(&self, id: EntityId) -> (Vector2, Vector2) {
        match self.sketch().entities[id] {
            Entity::Line { start, end } => (self.point(start), self.point(end)),
            // `validate` ensures a line; anything else is degenerate.
            _ => (Vector2::zeros(), Vector2::zeros()),
        }
    }

    /// Direction `end − start` of a line.
    fn direction(&self, id: EntityId) -> Vector2 {
        let (a, b) = self.line(id);
        b - a
    }

    /// Centre and radius of a circle or arc.
    fn circle(&self, id: EntityId) -> (Vector2, Real) {
        match self.sketch().entities[id] {
            Entity::Circle { center, radius } => {
                (self.point(center), self.radius_param(id, radius))
            }
            Entity::Arc { center, start, .. } => {
                let c = self.point(center);
                (c, (self.point(start) - c).norm())
            }
            // `validate` ensures a circle or arc.
            _ => (Vector2::zeros(), 0.0),
        }
    }

    /// Control points and knots (empty = clamped uniform) of a spline.
    fn spline(&self, id: EntityId) -> (Vec<Vector2>, Vec<Real>) {
        match &self.sketch().entities[id] {
            Entity::Spline { .. } => self.sketch().spline_def(id, &|p| self.point(p)),
            // `validate` ensures a spline.
            _ => (Vec::new(), Vec::new()),
        }
    }
}

/// [`Geo`] over the geometry as currently stored in the sketch.
struct Stored<'a>(&'a Sketch);

impl Geo for Stored<'_> {
    fn sketch(&self) -> &Sketch {
        self.0
    }
    fn point(&self, id: PointId) -> Vector2 {
        self.0.points[id].position.coords
    }
    fn radius_param(&self, _: EntityId, stored: Real) -> Real {
        stored
    }
}

impl Constraint {
    /// Human-readable name of the variant (used in UI and errors).
    pub fn name(&self) -> &'static str {
        match self {
            Constraint::Coincident { .. } => "Coincident",
            Constraint::PointOnLine { .. } => "Coincident (on line)",
            Constraint::PointOnCircle { .. } => "Coincident (on circle)",
            Constraint::PointOnSpline { .. } => "Coincident (on spline)",
            Constraint::Translated { .. } | Constraint::Rotated { .. } => "Pattern",
            Constraint::Midpoint { .. } => "Midpoint",
            Constraint::Collinear { .. } => "Collinear",
            Constraint::Concentric { .. } => "Concentric",
            Constraint::Fix { .. } => "Fix",
            Constraint::Horizontal { .. } | Constraint::HorizontalPoints { .. } => "Horizontal",
            Constraint::Vertical { .. } | Constraint::VerticalPoints { .. } => "Vertical",
            Constraint::Parallel { .. } => "Parallel",
            Constraint::Perpendicular { .. } => "Perpendicular",
            Constraint::Tangent { .. } => "Tangent",
            Constraint::Equal { .. } => "Equal",
            Constraint::Symmetric { .. } | Constraint::SymmetricOnArc { .. } => "Symmetric",
            Constraint::EqualOffset { .. } => "Offset",
            Constraint::Distance { .. } => "Distance",
            Constraint::HorizontalDistance { .. } => "Horizontal distance",
            Constraint::VerticalDistance { .. } => "Vertical distance",
            Constraint::PointLineDistance { .. } => "Point–line distance",
            Constraint::DiameterToAxis { .. } => "Diameter (to centerline)",
            Constraint::Angle { .. } => "Angle",
            Constraint::Radius { .. } => "Radius",
            Constraint::Diameter { .. } => "Diameter",
        }
    }

    /// Is this a dimension (has an editable value)?
    pub fn is_dimension(&self) -> bool {
        self.value().is_some()
    }

    /// The value of a dimension.
    pub fn value(&self) -> Option<Real> {
        match *self {
            Constraint::Distance { value, .. }
            | Constraint::HorizontalDistance { value, .. }
            | Constraint::VerticalDistance { value, .. }
            | Constraint::PointLineDistance { value, .. }
            | Constraint::DiameterToAxis { value, .. }
            | Constraint::Angle { value, .. }
            | Constraint::Radius { value, .. }
            | Constraint::Diameter { value, .. } => Some(value),
            _ => None,
        }
    }

    /// Mutable access to the value of a dimension.
    pub(crate) fn value_mut(&mut self) -> Option<&mut Real> {
        match self {
            Constraint::Distance { value, .. }
            | Constraint::HorizontalDistance { value, .. }
            | Constraint::VerticalDistance { value, .. }
            | Constraint::PointLineDistance { value, .. }
            | Constraint::DiameterToAxis { value, .. }
            | Constraint::Angle { value, .. }
            | Constraint::Radius { value, .. }
            | Constraint::Diameter { value, .. } => Some(value),
            _ => None,
        }
    }

    /// Does this constraint refer to point `p` directly?
    pub fn references_point(&self, p: PointId) -> bool {
        match *self {
            Constraint::Coincident { a, b }
            | Constraint::HorizontalPoints { a, b }
            | Constraint::VerticalPoints { a, b }
            | Constraint::Distance { a, b, .. }
            | Constraint::HorizontalDistance { a, b, .. }
            | Constraint::VerticalDistance { a, b, .. }
            | Constraint::Symmetric { a, b, .. } => a == p || b == p,
            Constraint::SymmetricOnArc { a, b, about, .. } => [a, b, about].contains(&p),
            Constraint::Translated {
                a,
                a2,
                b,
                b2,
                arc_center,
                ..
            } => [a, a2, b, b2].contains(&p) || arc_center == Some(p),
            Constraint::Rotated {
                center,
                a,
                a2,
                b,
                b2,
                arc_center,
            } => [center, a, a2, b, b2].contains(&p) || arc_center == Some(p),
            Constraint::PointOnLine { point, .. }
            | Constraint::PointOnCircle { point, .. }
            | Constraint::PointOnSpline { point, .. }
            | Constraint::Midpoint { point, .. }
            | Constraint::Fix { point, .. }
            | Constraint::PointLineDistance { point, .. }
            | Constraint::DiameterToAxis { point, .. } => point == p,
            _ => false,
        }
    }

    /// Does this constraint refer to entity `e`?
    pub fn references_entity(&self, e: EntityId) -> bool {
        match *self {
            Constraint::PointOnLine { line, .. }
            | Constraint::Midpoint { line, .. }
            | Constraint::Symmetric { line, .. }
            | Constraint::SymmetricOnArc { line, .. }
            | Constraint::PointLineDistance { line, .. }
            | Constraint::DiameterToAxis { line, .. }
            | Constraint::Horizontal { line }
            | Constraint::Vertical { line } => line == e,
            Constraint::PointOnCircle { circle, .. }
            | Constraint::Radius { circle, .. }
            | Constraint::Diameter { circle, .. } => circle == e,
            Constraint::PointOnSpline { spline, .. } => spline == e,
            Constraint::Parallel { a, b }
            | Constraint::Perpendicular { a, b }
            | Constraint::Collinear { a, b }
            | Constraint::Concentric { a, b }
            | Constraint::Equal { a, b }
            | Constraint::Angle { a, b, .. }
            | Constraint::Tangent { a, b, .. } => a == e || b == e,
            Constraint::EqualOffset {
                base_a,
                copy_a,
                base_b,
                copy_b,
            } => [base_a, copy_a, base_b, copy_b].contains(&e),
            _ => false,
        }
    }

    /// Checks that every referenced item exists and has the right kind, and
    /// that dimensional values are valid.
    pub(crate) fn validate(&self, sketch: &Sketch) -> Result<(), SketchError> {
        let point = |p: PointId| sketch.require_point(p);
        let entity = |e: EntityId, kind: Kind| {
            let ent = sketch.entities.get(e).ok_or(SketchError::UnknownEntity)?;
            let ok = match kind {
                Kind::Line => ent.is_line(),
                Kind::Circular => ent.is_circular(),
                Kind::LineOrCircular => ent.is_line() || ent.is_circular(),
                Kind::Spline => ent.is_spline(),
            };
            ok.then_some(())
                .ok_or(SketchError::WrongEntityKind(self.name()))
        };
        let positive = |v: Real, what| {
            (v.is_finite() && v > 0.0)
                .then_some(())
                .ok_or(SketchError::InvalidValue(what))
        };
        let distinct = |a: PointId, b: PointId| {
            (a != b)
                .then_some(())
                .ok_or(SketchError::InvalidValue("the two points must differ"))
        };

        match *self {
            Constraint::Coincident { a, b }
            | Constraint::HorizontalPoints { a, b }
            | Constraint::VerticalPoints { a, b } => {
                point(a)?;
                point(b)?;
                distinct(a, b)
            }
            Constraint::PointOnLine { point: p, line }
            | Constraint::Midpoint { point: p, line } => {
                point(p)?;
                entity(line, Kind::Line)
            }
            Constraint::PointOnCircle { point: p, circle } => {
                point(p)?;
                entity(circle, Kind::Circular)
            }
            Constraint::Translated {
                a,
                a2,
                b,
                b2,
                arc_center,
                ..
            } => {
                for q in [a, a2, b, b2].into_iter().chain(arc_center) {
                    point(q)?;
                }
                distinct(a, a2)?;
                distinct(b, b2)
            }
            Constraint::Rotated {
                center,
                a,
                a2,
                b,
                b2,
                arc_center,
            } => {
                for q in [center, a, a2, b, b2].into_iter().chain(arc_center) {
                    point(q)?;
                }
                distinct(a, a2)?;
                distinct(b, b2)
            }
            Constraint::PointOnSpline { point: p, spline } => {
                point(p)?;
                entity(spline, Kind::Spline)?;
                // A control point is not on the curve (except the ends,
                // which are shared, not constrained).
                (!sketch.entities[spline].points().contains(&p))
                    .then_some(())
                    .ok_or(SketchError::InvalidValue("the point defines the spline"))
            }
            Constraint::Fix { point: p, at } => {
                point(p)?;
                (at.x.is_finite() && at.y.is_finite())
                    .then_some(())
                    .ok_or(SketchError::InvalidValue("fix position must be finite"))
            }
            Constraint::Horizontal { line } | Constraint::Vertical { line } => {
                entity(line, Kind::Line)
            }
            Constraint::Parallel { a, b }
            | Constraint::Perpendicular { a, b }
            | Constraint::Collinear { a, b } => {
                entity(a, Kind::Line)?;
                entity(b, Kind::Line)?;
                (a != b)
                    .then_some(())
                    .ok_or(SketchError::WrongEntityKind(self.name()))
            }
            Constraint::Concentric { a, b } => {
                entity(a, Kind::Circular)?;
                entity(b, Kind::Circular)?;
                (a != b)
                    .then_some(())
                    .ok_or(SketchError::WrongEntityKind(self.name()))
            }
            Constraint::Tangent { a, b, .. } => {
                entity(a, Kind::LineOrCircular)?;
                entity(b, Kind::LineOrCircular)?;
                // At least one side must be circular (line–line tangency is meaningless).
                if sketch.entities[a].is_line() && sketch.entities[b].is_line() {
                    return Err(SketchError::WrongEntityKind(self.name()));
                }
                Ok(())
            }
            Constraint::Equal { a, b } => {
                entity(a, Kind::LineOrCircular)?;
                entity(b, Kind::LineOrCircular)?;
                let (ea, eb) = (&sketch.entities[a], &sketch.entities[b]);
                (a != b && (ea.is_line() == eb.is_line()))
                    .then_some(())
                    .ok_or(SketchError::WrongEntityKind(self.name()))
            }
            Constraint::SymmetricOnArc { a, b, line, about } => {
                point(about)?;
                point(a)?;
                point(b)?;
                distinct(a, b)?;
                entity(line, Kind::Line)
            }
            Constraint::Symmetric { a, b, line } => {
                point(a)?;
                point(b)?;
                distinct(a, b)?;
                entity(line, Kind::Line)
            }
            Constraint::EqualOffset {
                base_a,
                copy_a,
                base_b,
                copy_b,
            } => {
                for (base, copy) in [(base_a, copy_a), (base_b, copy_b)] {
                    entity(base, Kind::LineOrCircular)?;
                    entity(copy, Kind::LineOrCircular)?;
                    if sketch.entities[base].is_line() != sketch.entities[copy].is_line() {
                        return Err(SketchError::WrongEntityKind(self.name()));
                    }
                }
                Ok(())
            }
            Constraint::Distance { a, b, value }
            | Constraint::HorizontalDistance { a, b, value }
            | Constraint::VerticalDistance { a, b, value } => {
                point(a)?;
                point(b)?;
                distinct(a, b)?;
                // Zero distance is non-differentiable here; that is what Coincident is for.
                positive(value, "distance must be positive (use Coincident for 0)")
            }
            Constraint::PointLineDistance {
                point: p,
                line,
                value,
            }
            | Constraint::DiameterToAxis {
                point: p,
                line,
                value,
            } => {
                point(p)?;
                entity(line, Kind::Line)?;
                positive(value, "distance must be positive (use Coincident for 0)")
            }
            Constraint::Angle { a, b, value, .. } => {
                entity(a, Kind::Line)?;
                entity(b, Kind::Line)?;
                (value.is_finite() && value.abs() <= std::f64::consts::PI + 1e-12)
                    .then_some(())
                    .ok_or(SketchError::InvalidValue("angle must be within ±180°"))
            }
            Constraint::Radius { circle, value } | Constraint::Diameter { circle, value } => {
                entity(circle, Kind::Circular)?;
                positive(value, "radius/diameter must be positive")
            }
        }
    }

    /// Appends this constraint's residuals (zero ⇔ satisfied) to `out`.
    ///
    /// Assumes [`Constraint::validate`] passed; the solver validates every
    /// constraint before evaluating any residual.
    pub(crate) fn residuals(&self, g: &impl Geo, out: &mut Vec<Real>) {
        match *self {
            Constraint::Coincident { a, b } => {
                let d = g.point(a) - g.point(b);
                out.extend([d.x, d.y]);
            }
            Constraint::PointOnLine { point, line } => {
                let (p0, p1) = g.line(line);
                out.push(signed_distance(p0, p1, g.point(point)));
            }
            Constraint::PointOnCircle { point, circle } => {
                let (c, r) = g.circle(circle);
                out.push((g.point(point) - c).norm() - r);
            }
            Constraint::PointOnSpline { point, spline } => {
                let (cv, knots) = g.spline(spline);
                out.push(spline_signed_distance(&cv, &knots, g.point(point)));
            }
            Constraint::Translated {
                a,
                a2,
                b,
                b2,
                arc_center,
                offset,
            } => {
                let r = (g.point(a2) - g.point(a))
                    - (g.point(b2) - g.point(b))
                    - Vector2::new(offset[0], offset[1]);
                pattern_residual(g, r, a2, arc_center, out);
            }
            Constraint::Rotated {
                center,
                a,
                a2,
                b,
                b2,
                arc_center,
            } => {
                let c = g.point(center);
                let (u, v) = (g.point(b) - c, g.point(b2) - c);
                let n = safe_norm(&u) * safe_norm(&v);
                let (cos, sin) = (u.dot(&v) / n, u.perp(&v) / n);
                let w = g.point(a) - c;
                let rotated = Vector2::new(cos * w.x - sin * w.y, sin * w.x + cos * w.y);
                let r = (g.point(a2) - c) - rotated;
                pattern_residual(g, r, a2, arc_center, out);
            }
            Constraint::Midpoint { point, line } => {
                let (p0, p1) = g.line(line);
                let d = g.point(point) - (p0 + p1) / 2.0;
                out.extend([d.x, d.y]);
            }
            Constraint::Collinear { a, b } => {
                let (a0, a1) = g.line(a);
                let (b0, b1) = g.line(b);
                out.push(signed_distance(a0, a1, b0));
                out.push(signed_distance(a0, a1, b1));
            }
            Constraint::Concentric { a, b } => {
                let d = g.circle(a).0 - g.circle(b).0;
                out.extend([d.x, d.y]);
            }
            Constraint::Fix { point, at } => {
                let d = g.point(point) - at.coords;
                out.extend([d.x, d.y]);
            }
            Constraint::Horizontal { line } => out.push(g.direction(line).y),
            Constraint::Vertical { line } => out.push(g.direction(line).x),
            Constraint::HorizontalPoints { a, b } => out.push(g.point(b).y - g.point(a).y),
            Constraint::VerticalPoints { a, b } => out.push(g.point(b).x - g.point(a).x),
            Constraint::Parallel { a, b } => {
                let (da, db) = (g.direction(a), g.direction(b));
                out.push(da.perp(&db) / (safe_norm(&da) * safe_norm(&db)));
            }
            Constraint::Perpendicular { a, b } => {
                let (da, db) = (g.direction(a), g.direction(b));
                out.push(da.dot(&db) / (safe_norm(&da) * safe_norm(&db)));
            }
            Constraint::Tangent { a, b, internal } => {
                let (ea, eb) = (&g.sketch().entities[a], &g.sketch().entities[b]);
                // Tangent *at a shared endpoint*: "distance = radius" has a
                // zero gradient there (a double root — the line already
                // passes through a point of the circle), which blinds the
                // solver. Use the first-order condition instead: the radius
                // at the shared point is perpendicular to the line, or both
                // centres are collinear with the shared point.
                // The same holds for ends joined by a Coincident constraint;
                // each curve then uses its own end point.
                if let Some((pa, pb)) = joined_endpoints(g.sketch(), ea, eb) {
                    if ea.is_line() || eb.is_line() {
                        let (line, circle, q) = if ea.is_line() { (a, b, pb) } else { (b, a, pa) };
                        let d = g.direction(line);
                        let radial = g.point(q) - g.circle(circle).0;
                        out.push(radial.dot(&d) / (safe_norm(&radial) * safe_norm(&d)));
                    } else {
                        let (r1, r2) = (g.point(pa) - g.circle(a).0, g.point(pb) - g.circle(b).0);
                        out.push(r1.perp(&r2) / (safe_norm(&r1) * safe_norm(&r2)));
                    }
                    return;
                }
                // A line end held on the circle (PointOnCircle — the Line
                // tool's tangent snap): the same double root, the same cure.
                if ea.is_line() != eb.is_line() {
                    let (line, circle) = if ea.is_line() { (a, b) } else { (b, a) };
                    let ends = g.sketch().entities[line].endpoints();
                    let on = g.sketch().constraints().find_map(|(_, c)| match *c {
                        Constraint::PointOnCircle { point, circle: k }
                            if k == circle && ends.contains(&point) =>
                        {
                            Some(point)
                        }
                        _ => None,
                    });
                    if let Some(q) = on {
                        let d = g.direction(line);
                        let radial = g.point(q) - g.circle(circle).0;
                        out.push(radial.dot(&d) / (safe_norm(&radial) * safe_norm(&d)));
                        return;
                    }
                }
                if ea.is_line() || eb.is_line() {
                    let (line, circle) = if ea.is_line() { (a, b) } else { (b, a) };
                    let (p0, p1) = g.line(line);
                    let (c, r) = g.circle(circle);
                    out.push(signed_distance(p0, p1, c).abs() - r);
                } else {
                    let ((c1, r1), (c2, r2)) = (g.circle(a), g.circle(b));
                    let target = if internal { (r1 - r2).abs() } else { r1 + r2 };
                    out.push((c1 - c2).norm() - target);
                }
            }
            Constraint::Equal { a, b } => {
                if g.sketch().entities[a].is_line() {
                    out.push(g.direction(a).norm() - g.direction(b).norm());
                } else {
                    out.push(g.circle(a).1 - g.circle(b).1);
                }
            }
            Constraint::Symmetric { a, b, line } => {
                let (l0, l1) = g.line(line);
                let (pa, pb) = (g.point(a), g.point(b));
                let d = l1 - l0;
                // Midpoint on the line, and a→b perpendicular to it.
                out.push(signed_distance(l0, l1, (pa + pb) / 2.0));
                out.push((pb - pa).dot(&d) / safe_norm(&d));
            }
            Constraint::SymmetricOnArc { a, b, line, about } => {
                let (l0, l1) = g.line(line);
                let d = (l1 - l0) / safe_norm(&(l1 - l0));
                let v = g.point(a) - l0;
                let mirrored = l0 + d * (2.0 * v.dot(&d)) - v;
                pattern_residual(g, g.point(b) - mirrored, b, Some(about), out);
            }
            Constraint::EqualOffset {
                base_a,
                copy_a,
                base_b,
                copy_b,
            } => {
                // Magnitudes: a line's signed offset and an arc's radius change
                // have opposite signs for the same side of a chain.
                out.push(offset_of(g, base_a, copy_a).abs() - offset_of(g, base_b, copy_b).abs());
            }
            // Every remaining kind is a dimension (measured and valued);
            // the zero keeps the equation count should one ever not be.
            _ => out.push(
                self.measure_with(g)
                    .zip(self.value())
                    .map_or(0.0, |(m, v)| m - v),
            ),
        }
    }

    /// The current value of a dimension measured on the stored geometry
    /// (`None` for geometric constraints).
    pub fn measure(&self, sketch: &Sketch) -> Option<Real> {
        if self.validate(sketch).is_err() {
            return None;
        }
        self.measure_with(&Stored(sketch))
    }

    /// The angle residual wraps into (−π, π], so this returns the value that
    /// minimises it.
    fn measure_with(&self, g: &impl Geo) -> Option<Real> {
        Some(match *self {
            Constraint::Distance { a, b, .. } => (g.point(a) - g.point(b)).norm(),
            Constraint::HorizontalDistance { a, b, .. } => g.point(b).x - g.point(a).x,
            Constraint::VerticalDistance { a, b, .. } => g.point(b).y - g.point(a).y,
            Constraint::PointLineDistance { point, line, .. } => {
                let (p0, p1) = g.line(line);
                signed_distance(p0, p1, g.point(point)).abs()
            }
            Constraint::DiameterToAxis { point, line, .. } => {
                let (p0, p1) = g.line(line);
                2.0 * signed_distance(p0, p1, g.point(point)).abs()
            }
            Constraint::Angle {
                a,
                b,
                value,
                supplement,
            } => {
                let da = g.direction(a);
                let db = if supplement {
                    -g.direction(b)
                } else {
                    g.direction(b)
                };
                let angle = da.perp(&db).atan2(da.dot(&db));
                // Unwrap towards the target so the residual is continuous.
                value + wrap_angle(angle - value)
            }
            Constraint::Radius { circle, .. } => g.circle(circle).1,
            Constraint::Diameter { circle, .. } => 2.0 * g.circle(circle).1,
            _ => return None,
        })
    }
}

/// Offset of `copy` from `base` (see [`Constraint::EqualOffset`]).
fn offset_of(g: &impl Geo, base: EntityId, copy: EntityId) -> Real {
    if g.sketch().entities[base].is_line() {
        let (b0, b1) = g.line(base);
        signed_distance(b0, b1, g.line(copy).0)
    } else {
        g.circle(copy).1 - g.circle(base).1
    }
}

/// Pattern residual `r` at copied point `a2`: both components, or only the
/// one tangent to the copied arc around `arc_center`.
fn pattern_residual(
    g: &impl Geo,
    r: Vector2,
    a2: PointId,
    arc_center: Option<PointId>,
    out: &mut Vec<Real>,
) {
    match arc_center {
        Some(c) => {
            let radial = g.point(a2) - g.point(c);
            out.push(radial.perp(&r) / safe_norm(&radial));
        }
        None => out.extend([r.x, r.y]),
    }
}

/// A point that ends both entities (shared, not merely coincident).
fn shared_endpoint(a: &Entity, b: &Entity) -> Option<PointId> {
    let eb = b.endpoints();
    a.endpoints().into_iter().find(|p| eb.contains(p))
}

/// End points of `a` and `b` that are one point: shared, or joined by a
/// `Coincident` constraint. Returns (end of `a`, end of `b`).
fn joined_endpoints(sketch: &super::Sketch, a: &Entity, b: &Entity) -> Option<(PointId, PointId)> {
    if let Some(p) = shared_endpoint(a, b) {
        return Some((p, p));
    }
    let (ea, eb) = (a.endpoints(), b.endpoints());
    sketch.constraints().find_map(|(_, c)| match *c {
        Constraint::Coincident { a: p, b: q } if ea.contains(&p) && eb.contains(&q) => Some((p, q)),
        Constraint::Coincident { a: p, b: q } if ea.contains(&q) && eb.contains(&p) => Some((q, p)),
        _ => None,
    })
}

/// Residual that keeps an arc's end point on the circle through its start.
pub(crate) fn arc_residual(g: &impl Geo, entity: &Entity, out: &mut Vec<Real>) {
    if let Entity::Arc { center, start, end } = *entity {
        let c = g.point(center);
        out.push((g.point(end) - c).norm() - (g.point(start) - c).norm());
    }
}

/// Signed distance from `p` to a sketch spline with control points `cvs`
/// (positive = left of its direction), measured at the closest point.
///
/// Signed, not absolute: `|d|` has no usable derivative at 0, which is
/// exactly where the solver needs it. Only the distance (not the foot
/// parameter) is an output, so the closest-point search needs no extra
/// unknown in the solver: the derivative of the distance with respect to
/// the foot parameter vanishes at the optimum.
pub(crate) fn spline_signed_distance(cvs: &[Vector2], knots: &[Real], p: Vector2) -> Real {
    let (s, d) = spline_closest(cvs, knots, p);
    d.perp(&(p - s)) / safe_norm(&d)
}

/// Closest point on a sketch spline to `p` and the tangent there.
pub(crate) fn spline_closest(cvs: &[Vector2], knots: &[Real], p: Vector2) -> (Vector2, Vector2) {
    use crate::kernel::{BSplineCurve, Point3};
    let pts = cvs.iter().map(|v| Point3::new(v.x, v.y, 0.0)).collect();
    let Ok(curve) = BSplineCurve::sketch_spline(pts, knots) else {
        // Degenerate mid-iteration (coincident control points): use the chord.
        return (cvs[0], cvs[cvs.len() - 1] - cvs[0]);
    };
    let dom = curve.domain();
    let at = |t: Real| curve.point_at(t).coords.xy();
    let dist = |t: Real| (at(t) - p).norm_squared();
    // Coarse sampling, then golden-section search around the best sample.
    let n = 16 * cvs.len();
    let step = (dom.max - dom.min) / n as Real;
    let best = (0..=n)
        .map(|i| dom.min + step * i as Real)
        .min_by(|&a, &b| dist(a).total_cmp(&dist(b)))
        .unwrap_or(dom.min);
    let (mut a, mut b) = ((best - step).max(dom.min), (best + step).min(dom.max));
    const PHI: Real = 0.618_033_988_749_895;
    for _ in 0..60 {
        let (c, d) = (b - PHI * (b - a), a + PHI * (b - a));
        if dist(c) < dist(d) {
            b = d;
        } else {
            a = c;
        }
    }
    let t = (a + b) / 2.0;
    (at(t), curve.derivative_at(t).xy())
}

/// Signed perpendicular distance from `p` to the line through `a`, `b`.
fn signed_distance(a: Vector2, b: Vector2, p: Vector2) -> Real {
    let d = b - a;
    d.perp(&(p - a)) / safe_norm(&d)
}

/// Vector length guarded against zero (degenerate lines mid-iteration).
fn safe_norm(v: &Vector2) -> Real {
    v.norm().max(1e-12)
}

/// Wraps an angle into (−π, π].
pub(crate) fn wrap_angle(a: Real) -> Real {
    use std::f64::consts::{PI, TAU};
    let w = (a + PI).rem_euclid(TAU) - PI;
    if w <= -PI { w + TAU } else { w }
}

/// Serde helper: an all-zero offset is not written.
fn is_zero2(v: &[Real; 2]) -> bool {
    v[0] == 0.0 && v[1] == 0.0
}

#[cfg(test)]
mod tangent_on_curve_tests {
    use crate::sketch::{Constraint, Entity, Point2, Sketch};

    /// A line whose end lies on a circle (PointOnCircle) and touches it
    /// there (the tangent snap of the Line tool): no redundancy, and the
    /// line ends exactly at the touching point.
    #[test]
    fn line_tangent_with_its_end_on_the_circle() {
        let mut sk = Sketch::default();
        let c = sk.add_circle(Point2::new(0.0, 0.0), 10.0).unwrap();
        // Start outside, end roughly at the tangent point from (30, 0).
        let l = sk.add_line(Point2::new(30.0, 0.0), Point2::new(3.6, 9.2));
        let Some(Entity::Line { start, end }) = sk.entity(l).cloned() else {
            panic!("line")
        };
        sk.add_constraint(Constraint::PointOnCircle {
            point: end,
            circle: c,
        })
        .unwrap();
        sk.add_constraint(Constraint::Tangent {
            a: l,
            b: c,
            internal: false,
        })
        .unwrap();
        let r = sk.solve().unwrap();
        assert_eq!(r.redundant, 0, "{r:?}");
        assert_eq!(r.dof, 3 + 4 - 2, "{r:?}");
        let (p0, p1) = (
            sk.point(start).unwrap().position,
            sk.point(end).unwrap().position,
        );
        let Some(&Entity::Circle { center, radius }) = sk.entity(c) else {
            panic!("circle")
        };
        let radial = p1 - sk.point(center).unwrap().position;
        assert!((radial.norm() - radius).abs() < 1e-8, "{r:?} {p0:?} {p1:?}");
        assert!(
            radial.dot(&(p1 - p0)).abs() < 1e-6 * (p1 - p0).norm(),
            "touches at the end"
        );
        let _ = start;
    }
}
