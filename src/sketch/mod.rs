//! # 2D sketch mode and the geometric constraint solver
//!
//! A [`Sketch`] is a set of 2D entities (points, lines, circles, arcs) drawn
//! on a [`Plane`], plus [`Constraint`]s between them. Calling
//! [`Sketch::solve`] moves the geometry so that every constraint holds,
//! changing it as little as possible.
//!
//! ## How the solver sees a sketch
//!
//! Every free scalar is a **degree of freedom (DOF)**: the `x` and `y` of each
//! non-fixed point and the radius of each circle. Every constraint contributes
//! one or more **residuals** — scalar functions of the DOFs that are zero
//! exactly when the constraint is satisfied:
//!
//! | Constraint        | Residual(s)                                     |
//! |-------------------|-------------------------------------------------|
//! | Coincident        | `p.x − q.x`, `p.y − q.y`                        |
//! | Parallel          | `sin ∠(d₁, d₂) = (d₁ × d₂) / (‖d₁‖‖d₂‖)`        |
//! | Perpendicular     | `cos ∠(d₁, d₂) = (d₁ · d₂) / (‖d₁‖‖d₂‖)`        |
//! | Tangent (line, ○) | `dist(centre, line) − r`                        |
//! | Tangent (○, ○)    | `‖c₁ − c₂‖ − (r₁ + r₂)` or `− |r₁ − r₂|`        |
//!
//! Solving the sketch means driving the residual vector `F(x)` to zero, which
//! the solver does with Levenberg–Marquardt (a damped Gauss–Newton method).
//! The rank of the Jacobian at the solution tells us how many DOFs remain
//! (under-constrained) and how many equations are redundant (over-constrained),
//! which is what CAD UIs show as "fully defined" / "over-defined".
//!
//! ## Identity
//!
//! Like the kernel, a sketch stores its items in generational arenas and hands
//! out typed IDs ([`PointId`], [`EntityId`], [`ConstraintId`]). IDs are stable
//! across solves, serialisation and undo, so commands and AI agents can refer
//! to "line #3" reliably.

mod constraint;
mod corner;
mod dimparam;
mod modify;
mod pattern;
mod profile;
mod shapes;
mod solver;
mod spline;

pub use constraint::Constraint;

/// Wraps an angle into (−π, π] (for display code).
pub fn wrap_angle_pub(a: Real) -> Real {
    constraint::wrap_angle(a)
}
pub use corner::ChamferKind;
pub use dimparam::{DimDisplay, DimParam, EvalSize, TolKind, Tolerance};
pub use pattern::{InstanceOffset, PatternDirection, PatternInstance, PatternKind, SketchPattern};
pub use profile::{ProfileCurve, ProfileEdge, ProfileLoop, Region};
pub use shapes::Shape;
pub use solver::{DofAnalysis, PointFreedom, SolveReport, SolveStatus, SolverOptions};
pub use spline::SplineFit;

use nalgebra as na;
use serde::{Deserialize, Serialize};
use slotmap::{SecondaryMap, SlotMap, new_key_type};
use thiserror::Error;

use crate::kernel::{Plane, Point3, Real};

/// A position in sketch-plane coordinates `(u, v)`, in millimetres.
pub type Point2 = na::Point2<Real>;

/// A 2D displacement in sketch-plane coordinates.
pub type Vector2 = na::Vector2<Real>;

new_key_type! {
    /// Handle to a [`SketchPoint`].
    pub struct PointId;
    /// Handle to a sketch [`Entity`] (line, circle, arc).
    pub struct EntityId;
    /// Handle to a [`Constraint`].
    pub struct ConstraintId;
    /// Handle to a [`SketchPattern`].
    pub struct PatternId;
}

/// A point of the sketch. Lines, circles and arcs are defined by points, so
/// two entities that share a `PointId` are connected by construction.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SketchPoint {
    /// Current position on the sketch plane.
    pub position: Point2,
    /// A fixed point is not a DOF: the solver never moves it.
    pub fixed: bool,
    /// Inventor's point style: a plain sketch point or a Center Point
    /// (drawn as a cross; Hole takes centre points by default).
    #[serde(default, skip_serializing_if = "PointStyle::is_normal")]
    pub style: PointStyle,
}

/// The style of a sketch point (Inventor: Format → Center Point).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PointStyle {
    /// A plain sketch point.
    #[default]
    Normal,
    /// A centre point (hole centres).
    Center,
}

impl PointStyle {
    /// Whether it is the plain style (not saved).
    pub fn is_normal(&self) -> bool {
        *self == PointStyle::Normal
    }
}

/// A curve of the sketch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Entity {
    /// Straight segment from `start` to `end`.
    Line {
        /// First endpoint.
        start: PointId,
        /// Second endpoint.
        end: PointId,
    },
    /// Full circle. The radius is itself a DOF.
    Circle {
        /// Centre point.
        center: PointId,
        /// Radius (> 0), in mm.
        radius: Real,
    },
    /// Circular arc running counter-clockwise from `start` to `end`.
    ///
    /// Its radius is implied by `‖start − center‖`; the solver adds an
    /// implicit residual `‖end − center‖ − ‖start − center‖` so both endpoints
    /// stay on the same circle.
    Arc {
        /// Centre point.
        center: PointId,
        /// Start point (on the circle).
        start: PointId,
        /// End point (on the circle).
        end: PointId,
    },
    /// Control-vertex spline (Inventor's *Control Vertex Spline*): a clamped
    /// uniform B-spline of degree `min(3, n − 1)` over its control points. It
    /// starts at the first and ends at the last control point, tangent to the
    /// control frame there; the inner points pull the curve but are not on it.
    Spline {
        /// Control vertices, at least 2.
        points: Vec<PointId>,
    },
}

/// Line style of a sketch entity (Inventor: Sketch → Format panel).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Style {
    /// Ordinary geometry: forms profiles for features.
    #[default]
    Normal,
    /// Construction geometry: helps constrain, never part of a profile.
    Construction,
    /// Centerline: construction that also acts as a revolve axis and makes
    /// dimensions to it diametral.
    Centerline,
}

impl Entity {
    /// Is this a line?
    pub fn is_line(&self) -> bool {
        matches!(self, Entity::Line { .. })
    }

    /// Is this circular (a circle or an arc)?
    pub fn is_circular(&self) -> bool {
        matches!(self, Entity::Circle { .. } | Entity::Arc { .. })
    }

    /// Is this a spline?
    pub fn is_spline(&self) -> bool {
        matches!(self, Entity::Spline { .. })
    }

    /// All points this entity is defined by.
    pub fn points(&self) -> Vec<PointId> {
        match self {
            Entity::Line { start, end } => vec![*start, *end],
            Entity::Circle { center, .. } => vec![*center],
            Entity::Arc { center, start, end } => vec![*center, *start, *end],
            Entity::Spline { points } => points.clone(),
        }
    }

    /// The points where the entity ends (for chaining and tangent arcs):
    /// line and spline ends, arc start/end; none for circles.
    pub fn endpoints(&self) -> Vec<PointId> {
        match self {
            Entity::Line { start, end } | Entity::Arc { start, end, .. } => vec![*start, *end],
            Entity::Spline { points } => vec![points[0], points[points.len() - 1]],
            Entity::Circle { .. } => Vec::new(),
        }
    }
}

/// Errors when editing or solving a sketch.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SketchError {
    /// A [`PointId`] that is not (or no longer) in the sketch.
    #[error("unknown point")]
    UnknownPoint,
    /// An [`EntityId`] that is not (or no longer) in the sketch.
    #[error("unknown entity")]
    UnknownEntity,
    /// A [`ConstraintId`] that is not (or no longer) in the sketch.
    #[error("unknown constraint")]
    UnknownConstraint,
    /// The constraint does not apply to these kinds of entities
    /// (e.g. `Horizontal` on a circle).
    #[error("{0} constraint cannot be applied to these entities")]
    WrongEntityKind(&'static str),
    /// A numeric value is out of range.
    #[error("invalid value: {0}")]
    InvalidValue(&'static str),
}

/// A 2D sketch on a plane: points, entities and constraints.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sketch {
    /// The plane the sketch lives on; maps `(u, v)` to 3D.
    pub plane: Plane,
    points: SlotMap<PointId, SketchPoint>,
    entities: SlotMap<EntityId, Entity>,
    constraints: SlotMap<ConstraintId, Constraint>,
    /// Non-normal entity styles (absent = [`Style::Normal`]).
    #[serde(default)]
    styles: SecondaryMap<EntityId, Style>,
    /// Driven (reference) dimensions: shown with their measured value, not
    /// used by the solver.
    #[serde(default, with = "unit_set")]
    driven: SecondaryMap<ConstraintId, ()>,
    /// Where each dimension's text sits, as an offset from the dimension's
    /// anchor ([`Sketch::dimension_anchor`]), so it follows the geometry.
    /// Absent = at the anchor.
    #[serde(default, skip_serializing_if = "SecondaryMap::is_empty")]
    dim_labels: SecondaryMap<ConstraintId, Vector2>,
    /// Parameter name, equation and tolerance of the dimensions.
    #[serde(default, skip_serializing_if = "SecondaryMap::is_empty")]
    dim_params: SecondaryMap<ConstraintId, DimParam>,
    /// Patterns as editable objects (Edit Pattern, Suppress, Delete).
    #[serde(default, skip_serializing_if = "SlotMap::is_empty")]
    patterns: SlotMap<PatternId, SketchPattern>,
    /// Interpolation splines (through their points) and their handles;
    /// other splines are control-vertex splines.
    #[serde(default, skip_serializing_if = "SecondaryMap::is_empty")]
    interp: SecondaryMap<EntityId, spline::SplineFit>,
    /// Circles whose radius is not a degree of freedom (projected circles:
    /// the radius follows their source, see `part::link`).
    #[serde(default, skip_serializing_if = "SecondaryMap::is_empty")]
    fixed_radius: SecondaryMap<EntityId, bool>,
}

impl Default for Sketch {
    /// An empty sketch on the world XY plane.
    fn default() -> Self {
        Self::new(Plane::xy())
    }
}

impl Sketch {
    /// An empty sketch on `plane`.
    pub fn new(plane: Plane) -> Self {
        Self {
            plane,
            points: SlotMap::with_key(),
            entities: SlotMap::with_key(),
            constraints: SlotMap::with_key(),
            styles: SecondaryMap::new(),
            driven: SecondaryMap::new(),
            dim_labels: SecondaryMap::new(),
            dim_params: SecondaryMap::new(),
            patterns: SlotMap::with_key(),
            interp: SecondaryMap::new(),
            fixed_radius: SecondaryMap::new(),
        }
    }

    // ---- read access -------------------------------------------------------

    /// Looks up a point.
    pub fn point(&self, id: PointId) -> Option<&SketchPoint> {
        self.points.get(id)
    }
    /// Looks up an entity.
    pub fn entity(&self, id: EntityId) -> Option<&Entity> {
        self.entities.get(id)
    }
    /// Looks up a constraint.
    pub fn constraint(&self, id: ConstraintId) -> Option<&Constraint> {
        self.constraints.get(id)
    }
    /// Iterates over all points.
    pub fn points(&self) -> impl Iterator<Item = (PointId, &SketchPoint)> {
        self.points.iter()
    }
    /// Iterates over all entities.
    pub fn entities(&self) -> impl Iterator<Item = (EntityId, &Entity)> {
        self.entities.iter()
    }
    /// Iterates over all constraints.
    pub fn constraints(&self) -> impl Iterator<Item = (ConstraintId, &Constraint)> {
        self.constraints.iter()
    }

    /// Style of an entity.
    pub fn style(&self, id: EntityId) -> Style {
        self.styles.get(id).copied().unwrap_or_default()
    }

    /// Sets the style of an entity.
    pub fn set_style(&mut self, id: EntityId, style: Style) -> Result<(), SketchError> {
        if !self.entities.contains_key(id) {
            return Err(SketchError::UnknownEntity);
        }
        if style == Style::Normal {
            self.styles.remove(id);
        } else {
            self.styles.insert(id, style);
        }
        Ok(())
    }

    /// Is this constraint a driven (reference) dimension?
    pub fn is_driven(&self, id: ConstraintId) -> bool {
        self.driven.contains_key(id)
    }

    /// Makes a dimension driven (reference) or driving again.
    pub fn set_driven(&mut self, id: ConstraintId, driven: bool) -> Result<(), SketchError> {
        let c = self
            .constraints
            .get(id)
            .ok_or(SketchError::UnknownConstraint)?;
        if !c.is_dimension() {
            return Err(SketchError::WrongEntityKind(c.name()));
        }
        if driven {
            self.driven.insert(id, ());
        } else {
            self.driven.remove(id);
        }
        Ok(())
    }

    /// Current value of a dimension constraint as measured on the geometry
    /// (mm, or radians for angles).
    pub fn measure(&self, id: ConstraintId) -> Option<Real> {
        self.constraints.get(id)?.measure(self)
    }

    /// Endpoints of a line.
    pub fn line_geometry(&self, id: EntityId) -> Option<(Point2, Point2)> {
        match self.entities.get(id)? {
            Entity::Line { start, end } => Some((
                self.points.get(*start)?.position,
                self.points.get(*end)?.position,
            )),
            _ => None,
        }
    }

    /// Centre and radius of a circle or arc.
    pub fn circle_geometry(&self, id: EntityId) -> Option<(Point2, Real)> {
        self.circle_now(id)
    }

    /// The B-spline of a spline entity (in sketch coordinates, z = 0).
    pub fn spline_curve(&self, id: EntityId) -> Option<crate::kernel::BSplineCurve> {
        let Entity::Spline { points } = self.entities.get(id)? else {
            return None;
        };
        if points.iter().any(|p| !self.points.contains_key(*p)) {
            return None;
        }
        let (cv, knots) = self.spline_def(id, &|p| self.points[p].position.coords);
        let cvs: Vec<Point3> = cv.iter().map(|v| Point3::new(v.x, v.y, 0.0)).collect();
        crate::kernel::BSplineCurve::sketch_spline(cvs, &knots).ok()
    }

    /// Holds (or frees) a circle's radius: a held radius is no degree of
    /// freedom, the solver keeps it (projected circles).
    pub fn set_radius_fixed(&mut self, id: EntityId, fixed: bool) -> Result<(), SketchError> {
        match self.entities.get(id) {
            Some(Entity::Circle { .. }) => {}
            Some(_) => return Err(SketchError::WrongEntityKind("set_radius_fixed")),
            None => return Err(SketchError::UnknownEntity),
        }
        if fixed {
            self.fixed_radius.insert(id, true);
        } else {
            self.fixed_radius.remove(id);
        }
        Ok(())
    }

    /// Whether a circle's radius is held (see [`Sketch::set_radius_fixed`]).
    pub fn is_radius_fixed(&self, id: EntityId) -> bool {
        self.fixed_radius.contains_key(id)
    }

    /// Sets a circle's radius (> 0) directly; call [`Sketch::solve`] after.
    pub fn set_circle_radius(&mut self, id: EntityId, r: Real) -> Result<(), SketchError> {
        if !(r.is_finite() && r > 0.0) {
            return Err(SketchError::InvalidValue("circle radius must be positive"));
        }
        match self.entities.get_mut(id) {
            Some(Entity::Circle { radius, .. }) => {
                *radius = r;
                Ok(())
            }
            Some(_) => Err(SketchError::WrongEntityKind("set_circle_radius")),
            None => Err(SketchError::UnknownEntity),
        }
    }

    /// The `(start, end)` points of a line entity.
    pub fn line_points(&self, line: EntityId) -> Result<(PointId, PointId), SketchError> {
        match self.entities.get(line) {
            Some(&Entity::Line { start, end }) => Ok((start, end)),
            Some(_) => Err(SketchError::WrongEntityKind("line_points")),
            None => Err(SketchError::UnknownEntity),
        }
    }

    /// Maps a sketch point to 3D model space via the sketch plane.
    pub fn to_world(&self, p: &Point2) -> Point3 {
        self.plane.point_at(p.x, p.y)
    }

    // ---- construction ------------------------------------------------------

    /// Adds a free point.
    pub fn add_point(&mut self, position: Point2) -> PointId {
        self.points.insert(SketchPoint {
            position,
            fixed: false,
            style: PointStyle::Normal,
        })
    }

    /// Adds a Center Point (Inventor's point style for hole centres).
    pub fn add_center_point(&mut self, position: Point2) -> PointId {
        self.points.insert(SketchPoint {
            position,
            fixed: false,
            style: PointStyle::Center,
        })
    }

    /// Sets the style of a point (Format → Center Point).
    pub fn set_point_style(
        &mut self,
        point: PointId,
        style: PointStyle,
    ) -> Result<(), SketchError> {
        self.points
            .get_mut(point)
            .map(|p| p.style = style)
            .ok_or(SketchError::UnknownPoint)
    }

    /// The Center Points of the sketch.
    pub fn center_points(&self) -> Vec<PointId> {
        self.points()
            .filter(|(_, p)| p.style == PointStyle::Center)
            .map(|(id, _)| id)
            .collect()
    }

    /// Adds a point the solver will never move (an anchor).
    pub fn add_fixed_point(&mut self, position: Point2) -> PointId {
        self.points.insert(SketchPoint {
            position,
            fixed: true,
            style: PointStyle::Normal,
        })
    }

    /// Fixes or frees an existing point.
    pub fn set_fixed(&mut self, point: PointId, fixed: bool) -> Result<(), SketchError> {
        self.points
            .get_mut(point)
            .ok_or(SketchError::UnknownPoint)?
            .fixed = fixed;
        Ok(())
    }

    /// Moves a point (e.g. while the user drags it). Call [`Sketch::solve`]
    /// afterwards to restore the constraints.
    pub fn move_point(&mut self, point: PointId, position: Point2) -> Result<(), SketchError> {
        self.points
            .get_mut(point)
            .ok_or(SketchError::UnknownPoint)?
            .position = position;
        Ok(())
    }

    /// Adds a line between two existing points (shared points = connected
    /// geometry, no `Coincident` constraint needed).
    pub fn add_line_between(
        &mut self,
        start: PointId,
        end: PointId,
    ) -> Result<EntityId, SketchError> {
        self.require_point(start)?;
        self.require_point(end)?;
        if start == end {
            return Err(SketchError::InvalidValue(
                "line endpoints must be different points",
            ));
        }
        Ok(self.entities.insert(Entity::Line { start, end }))
    }

    /// Adds a line with two new endpoints.
    pub fn add_line(&mut self, start: Point2, end: Point2) -> EntityId {
        let (a, b) = (self.add_point(start), self.add_point(end));
        self.entities.insert(Entity::Line { start: a, end: b })
    }

    /// Adds a circle around a new centre point.
    pub fn add_circle(&mut self, center: Point2, radius: Real) -> Result<EntityId, SketchError> {
        let center = self.add_point(center);
        self.add_circle_at(center, radius)
    }

    /// Adds a circle around an existing point.
    pub fn add_circle_at(
        &mut self,
        center: PointId,
        radius: Real,
    ) -> Result<EntityId, SketchError> {
        self.require_point(center)?;
        if !(radius.is_finite() && radius > 0.0) {
            return Err(SketchError::InvalidValue("circle radius must be positive"));
        }
        Ok(self.entities.insert(Entity::Circle { center, radius }))
    }

    /// Adds a counter-clockwise arc defined by three existing points.
    pub fn add_arc(
        &mut self,
        center: PointId,
        start: PointId,
        end: PointId,
    ) -> Result<EntityId, SketchError> {
        for p in [center, start, end] {
            self.require_point(p)?;
        }
        if center == start || center == end || start == end {
            return Err(SketchError::InvalidValue("arc needs three distinct points"));
        }
        Ok(self.entities.insert(Entity::Arc { center, start, end }))
    }

    /// Adds a control-vertex spline through existing points (≥ 2, distinct
    /// neighbours).
    pub fn add_spline(&mut self, points: Vec<PointId>) -> Result<EntityId, SketchError> {
        if points.len() < 2 {
            return Err(SketchError::InvalidValue(
                "a spline needs at least 2 control points",
            ));
        }
        for p in &points {
            self.require_point(*p)?;
        }
        if points.windows(2).any(|w| w[0] == w[1]) {
            return Err(SketchError::InvalidValue(
                "consecutive spline points must differ",
            ));
        }
        Ok(self.entities.insert(Entity::Spline { points }))
    }

    /// Adds a constraint after checking that it references live items of the
    /// right kinds and has sensible values.
    pub fn add_constraint(&mut self, constraint: Constraint) -> Result<ConstraintId, SketchError> {
        constraint.validate(self)?;
        Ok(self.constraints.insert(constraint))
    }

    /// Adds a tangency, choosing internal vs. external contact for two
    /// circles from the current geometry (whichever is nearer to true), which
    /// is what the user means when they click two circles.
    pub fn add_tangent(&mut self, a: EntityId, b: EntityId) -> Result<ConstraintId, SketchError> {
        let internal = match (self.circle_now(a), self.circle_now(b)) {
            (Some((c1, r1)), Some((c2, r2))) => {
                let d = (c1 - c2).norm();
                (d - (r1 - r2).abs()).abs() < (d - (r1 + r2)).abs()
            }
            _ => false,
        };
        self.add_constraint(Constraint::Tangent { a, b, internal })
    }

    /// Changes the value of a dimension (any [`Constraint::is_dimension`]
    /// kind; angles in radians within ±π, lengths > 0). Call
    /// [`Sketch::solve`] afterwards to apply it to the geometry.
    pub fn set_dimension(&mut self, id: ConstraintId, new_value: Real) -> Result<(), SketchError> {
        let c = self
            .constraints
            .get_mut(id)
            .ok_or(SketchError::UnknownConstraint)?;
        let is_angle = matches!(c, Constraint::Angle { .. });
        let ok = new_value.is_finite()
            && if is_angle {
                new_value.abs() <= std::f64::consts::PI + 1e-12
            } else {
                new_value > 0.0
            };
        if !ok {
            return Err(SketchError::InvalidValue(if is_angle {
                "angle must be within ±180°"
            } else {
                "dimension must be positive"
            }));
        }
        let name = c.name();
        *c.value_mut().ok_or(SketchError::WrongEntityKind(name))? = new_value;
        Ok(())
    }

    /// The point a dimension's text is placed relative to: the middle of
    /// the measured points (linear), the centre (radius, diameter), the
    /// vertex of the two lines (angle). `None` for non-dimensions.
    pub fn dimension_anchor(&self, id: ConstraintId) -> Option<Point2> {
        let pos = |p: PointId| self.points.get(p).map(|q| q.position);
        Some(match *self.constraints.get(id)? {
            Constraint::Distance { a, b, .. }
            | Constraint::HorizontalDistance { a, b, .. }
            | Constraint::VerticalDistance { a, b, .. } => na::center(&pos(a)?, &pos(b)?),
            Constraint::PointLineDistance { point, line, .. }
            | Constraint::DiameterToAxis { point, line, .. } => {
                let p = pos(point)?;
                let (l0, l1) = self.line_geometry(line)?;
                let d = (l1 - l0).try_normalize(1e-12)?;
                na::center(&p, &(l0 + d * (p - l0).dot(&d)))
            }
            Constraint::Radius { circle, .. } | Constraint::Diameter { circle, .. } => {
                self.circle_geometry(circle)?.0
            }
            Constraint::Angle { a, b, .. } => {
                let (a0, a1) = self.line_geometry(a)?;
                let (b0, b1) = self.line_geometry(b)?;
                let (r, q) = (a1 - a0, b1 - b0);
                let den = r.perp(&q);
                if den.abs() < 1e-12 {
                    na::center(&na::center(&a0, &a1), &na::center(&b0, &b1))
                } else {
                    a0 + r * ((b0 - a0).perp(&q) / den)
                }
            }
            _ => return None,
        })
    }

    /// The angle an angle dimension placed at `place` measures, and whether
    /// it is the supplement (to `b` run backwards): Inventor lets the
    /// quadrant of the placement pick θ or 180° − θ.
    pub fn angle_for_placement(
        &self,
        a: EntityId,
        b: EntityId,
        place: Point2,
    ) -> Option<(Real, bool)> {
        let (a0, a1) = self.line_geometry(a)?;
        let (b0, b1) = self.line_geometry(b)?;
        let (ua, ub) = (a1 - a0, b1 - b0);
        let den = ua.perp(&ub);
        if den.abs() < 1e-12 {
            return None;
        }
        let x = a0 + ua * ((b0 - a0).perp(&ub) / den);
        let m = den.atan2(ua.dot(&ub));
        let la = (place - x).y.atan2((place - x).x);
        let start = ua.y.atan2(ua.x);
        // Is direction `t` inside the sector from `s` sweeping by `w`?
        let inside = |s: Real, w: Real| {
            let d = constraint::wrap_angle(la - s);
            if w >= 0.0 {
                (0.0..=w).contains(&d)
            } else {
                (w..=0.0).contains(&d)
            }
        };
        let pi = std::f64::consts::PI;
        if inside(start, m) || inside(start + pi, m) {
            Some((m, false))
        } else {
            Some((constraint::wrap_angle(m - pi), true))
        }
    }

    /// **Flip Direction** of a dimension (Inventor): the geometry goes to
    /// the other side — a horizontal/vertical distance swaps its points, a
    /// point–line distance mirrors the point over the line, an angle changes
    /// sign. Other kinds are refused. On failure nothing changes.
    pub fn flip_dimension(&mut self, id: ConstraintId) -> Result<(), SketchError> {
        let before = self.clone();
        let c = self
            .constraints
            .get_mut(id)
            .ok_or(SketchError::UnknownConstraint)?;
        match c {
            Constraint::HorizontalDistance { a, b, .. }
            | Constraint::VerticalDistance { a, b, .. } => std::mem::swap(a, b),
            Constraint::Angle { value, .. } => *value = -*value,
            Constraint::PointLineDistance { point, line, .. } => {
                let (point, line) = (*point, *line);
                let (l0, l1) = self.line_geometry(line).ok_or(SketchError::UnknownEntity)?;
                let p = self.point(point).ok_or(SketchError::UnknownPoint)?.position;
                let d = (l1 - l0).normalize();
                let foot = l0 + d * (p - l0).dot(&d);
                self.move_point(point, foot + (foot - p))?;
            }
            _ => {
                return Err(SketchError::InvalidValue(
                    "Flip Direction applies to horizontal/vertical, point–line and angle dimensions",
                ));
            }
        }
        if !matches!(self.solve(), Ok(r) if r.status == SolveStatus::Solved) {
            *self = before;
            return Err(SketchError::InvalidValue(
                "the flipped geometry conflicts with the other constraints",
            ));
        }
        Ok(())
    }

    /// Where the text of dimension `id` is (sketch coordinates).
    pub fn dimension_label(&self, id: ConstraintId) -> Option<Point2> {
        let anchor = self.dimension_anchor(id)?;
        Some(
            anchor
                + self
                    .dim_labels
                    .get(id)
                    .copied()
                    .unwrap_or_else(Vector2::zeros),
        )
    }

    /// Has the text of `id` been placed (vs. the default at its anchor)?
    pub fn has_dimension_label(&self, id: ConstraintId) -> bool {
        self.dim_labels.contains_key(id)
    }

    /// Puts the text of dimension `id` at `at` (kept relative to its anchor,
    /// so it follows the geometry); `None` resets it to the default.
    pub fn set_dimension_label(&mut self, id: ConstraintId, at: Option<Point2>) {
        match at.zip(self.dimension_anchor(id)) {
            Some((p, anchor)) => {
                self.dim_labels.insert(id, p - anchor);
            }
            None => {
                self.dim_labels.remove(id);
            }
        }
    }

    /// Makes every driving dimension temporarily driven, so geometry can
    /// move freely (Relax mode, Stretch); returns them for
    /// [`Sketch::remeasure_dimensions`].
    pub fn relax_dimensions(&mut self) -> Vec<ConstraintId> {
        self.relax_dimensions_where(|_| true)
    }

    /// [`Sketch::relax_dimensions`] for the driving dimensions that
    /// `which` selects.
    pub fn relax_dimensions_where(
        &mut self,
        which: impl Fn(&Constraint) -> bool,
    ) -> Vec<ConstraintId> {
        let ids: Vec<ConstraintId> = self
            .constraints
            .iter()
            .filter(|(id, c)| c.is_dimension() && !self.driven.contains_key(*id) && which(c))
            .map(|(id, _)| id)
            .collect();
        for &id in &ids {
            self.driven.insert(id, ());
        }
        ids
    }

    /// Sets the relaxed dimensions to their measured values and makes them
    /// driving again.
    pub fn remeasure_dimensions(&mut self, ids: &[ConstraintId]) {
        for &id in ids {
            if let Some(v) = self.measure(id) {
                let v = if matches!(self.constraints.get(id), Some(Constraint::Angle { .. })) {
                    (v + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU)
                        - std::f64::consts::PI
                } else {
                    v
                };
                let _ = self.set_dimension(id, v);
            }
            self.driven.remove(id);
        }
    }

    /// After a Relax-mode drag (Inventor): relaxed dimensions the drag had
    /// to change stay **driven** (reference, showing the new value); the
    /// others become driving again with their old value.
    pub fn settle_relaxed_dimensions(&mut self, ids: &[ConstraintId]) {
        for &id in ids {
            let (Some(old), Some(now)) = (
                self.constraints.get(id).and_then(Constraint::value),
                self.measure(id),
            ) else {
                continue;
            };
            let is_angle = matches!(self.constraints.get(id), Some(Constraint::Angle { .. }));
            let changed = if is_angle {
                constraint::wrap_angle(now - old).abs() > 1e-9
            } else {
                (now - old).abs() > 1e-9 * (1.0 + old.abs())
            };
            if changed {
                let now = if is_angle {
                    old + constraint::wrap_angle(now - old)
                } else {
                    now
                };
                if let Some(v) = self.constraints.get_mut(id).and_then(Constraint::value_mut) {
                    *v = now;
                }
            } else {
                self.driven.remove(id);
            }
        }
    }

    /// Removes a constraint, returning it.
    pub fn remove_constraint(&mut self, id: ConstraintId) -> Result<Constraint, SketchError> {
        self.driven.remove(id);
        self.dim_labels.remove(id);
        self.dim_params.remove(id);
        self.constraints
            .remove(id)
            .ok_or(SketchError::UnknownConstraint)
    }

    /// Removes an entity and every constraint that refers to it. Its points
    /// stay (they may be shared with other entities).
    pub fn remove_entity(&mut self, id: EntityId) -> Result<Entity, SketchError> {
        let entity = self.entities.remove(id).ok_or(SketchError::UnknownEntity)?;
        self.styles.remove(id);
        self.interp.remove(id);
        self.fixed_radius.remove(id);
        let doomed: Vec<ConstraintId> = self
            .constraints
            .iter()
            .filter(|(_, c)| c.references_entity(id))
            .map(|(cid, _)| cid)
            .collect();
        for cid in doomed {
            self.remove_constraint(cid)?;
        }
        Ok(entity)
    }

    /// Removes a point together with the entities built on it and all
    /// constraints referring to either.
    pub fn remove_point(&mut self, id: PointId) -> Result<(), SketchError> {
        self.require_point(id)?;
        let users: Vec<EntityId> = self
            .entities
            .iter()
            .filter(|(_, e)| e.points().contains(&id))
            .map(|(eid, _)| eid)
            .collect();
        for e in users {
            self.remove_entity(e)?;
        }
        let doomed: Vec<ConstraintId> = self
            .constraints
            .iter()
            .filter(|(_, c)| c.references_point(id))
            .map(|(cid, _)| cid)
            .collect();
        for cid in doomed {
            self.remove_constraint(cid)?;
        }
        self.points.remove(id);
        Ok(())
    }

    /// Re-expresses every point through `f` (e.g. into a new coordinate
    /// system). Circle radii are unchanged, so `f` must be a rigid motion.
    pub fn map_points(&mut self, mut f: impl FnMut(Point2) -> Point2) {
        for p in self.points.values_mut() {
            p.position = f(p.position);
        }
    }

    /// Which geometry can still move and how (Inventor's "Show All Degrees
    /// of Freedom"): per point fixed / along a direction / free, curves that
    /// can rotate, circles with free radius, fully constrained entities.
    pub fn dof_analysis(&self) -> DofAnalysis {
        solver::dof_analysis(self)
    }

    /// Solves the sketch with default [`SolverOptions`].
    ///
    /// On success the geometry is updated in place. If the constraints cannot
    /// be satisfied the sketch is left **unchanged** and the report's status
    /// is [`SolveStatus::Failed`].
    pub fn solve(&mut self) -> Result<SolveReport, SketchError> {
        self.solve_with(&SolverOptions::default())
    }

    /// Solves the sketch with explicit options (see [`Sketch::solve`]).
    pub fn solve_with(&mut self, options: &SolverOptions) -> Result<SolveReport, SketchError> {
        solver::solve(self, options)
    }

    // ---- helpers -----------------------------------------------------------

    fn require_point(&self, id: PointId) -> Result<(), SketchError> {
        self.points
            .contains_key(id)
            .then_some(())
            .ok_or(SketchError::UnknownPoint)
    }

    /// Centre and radius of a circle/arc from the stored geometry.
    fn circle_now(&self, id: EntityId) -> Option<(Point2, Real)> {
        let pos = |p: PointId| self.points.get(p).map(|p| p.position);
        match *self.entities.get(id)? {
            Entity::Circle { center, radius } => Some((pos(center)?, radius)),
            Entity::Arc { center, start, .. } => {
                let c = pos(center)?;
                Some((c, (pos(start)? - c).norm()))
            }
            Entity::Line { .. } | Entity::Spline { .. } => None,
        }
    }
}

/// Serde for a `SecondaryMap<K, ()>` (a set of keys). slotmap writes a
/// unit value as `null`, the same as an empty slot, and then refuses to read
/// the map back ("inconsistent occupation in Slot"): a sketch with a driven
/// dimension could be saved but not opened. Written as a map of `true`
/// (same layout), read by slot version, so files written the old way open.
mod unit_set {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use slotmap::{Key, KeyData, SecondaryMap};

    #[derive(Deserialize)]
    struct Slot {
        #[serde(default)]
        #[allow(dead_code)]
        value: Option<serde::de::IgnoredAny>,
        version: u32,
    }

    pub fn serialize<K: Key, S: Serializer>(
        set: &SecondaryMap<K, ()>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        let mut m: SecondaryMap<K, bool> = SecondaryMap::new();
        for k in set.keys() {
            m.insert(k, true);
        }
        m.serialize(s)
    }

    pub fn deserialize<'de, K: Key, D: Deserializer<'de>>(
        d: D,
    ) -> Result<SecondaryMap<K, ()>, D::Error> {
        let slots: Vec<Slot> = Vec::deserialize(d)?;
        let mut set = SecondaryMap::new();
        for (idx, slot) in slots.iter().enumerate().skip(1) {
            if slot.version % 2 == 1 {
                let ffi = (u64::from(slot.version) << 32) | idx as u64;
                set.insert(K::from(KeyData::from_ffi(ffi)), ());
            }
        }
        Ok(set)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    /// A driven dimension survives save/load (slotmap wrote the unit
    /// value as `null` and refused to read it back), also from a file
    /// written the old way.
    #[test]
    fn driven_dimension_round_trip() {
        let mut s = Sketch::default();
        let a = s.add_point(Point2::new(0.0, 0.0));
        let b = s.add_point(Point2::new(10.0, 0.0));
        let c = s
            .add_constraint(Constraint::Distance { a, b, value: 10.0 })
            .unwrap();
        let line = s.add_line_between(a, b).unwrap();
        let d = s.add_constraint(Constraint::Horizontal { line }).unwrap();
        s.set_driven(c, true).unwrap();
        let json = serde_json::to_string(&s).unwrap();
        let back: Sketch = serde_json::from_str(&json).unwrap();
        assert!(back.is_driven(c) && !back.is_driven(d));
        assert_eq!(serde_json::to_string(&back).unwrap(), json);
        let old = json.replace("\"value\":true", "\"value\":null");
        assert_ne!(old, json);
        let back: Sketch = serde_json::from_str(&old).unwrap();
        assert!(back.is_driven(c) && !back.is_driven(d));
    }

    fn p(x: Real, y: Real) -> Point2 {
        Point2::new(x, y)
    }

    fn pos(s: &Sketch, id: PointId) -> Point2 {
        s.point(id).unwrap().position
    }

    /// A slightly crooked quadrilateral with shared corner points.
    fn rough_rectangle(s: &mut Sketch) -> ([PointId; 4], [EntityId; 4]) {
        let c = [
            s.add_point(p(0.0, 0.3)),
            s.add_point(p(9.7, -0.2)),
            s.add_point(p(10.4, 5.1)),
            s.add_point(p(0.2, 4.8)),
        ];
        let l = [
            s.add_line_between(c[0], c[1]).unwrap(), // bottom
            s.add_line_between(c[1], c[2]).unwrap(), // right
            s.add_line_between(c[2], c[3]).unwrap(), // top
            s.add_line_between(c[3], c[0]).unwrap(), // left
        ];
        s.add_constraint(Constraint::Horizontal { line: l[0] })
            .unwrap();
        s.add_constraint(Constraint::Horizontal { line: l[2] })
            .unwrap();
        s.add_constraint(Constraint::Vertical { line: l[1] })
            .unwrap();
        s.add_constraint(Constraint::Vertical { line: l[3] })
            .unwrap();
        (c, l)
    }

    #[test]
    fn geometric_constraints_of_the_constrain_panel() {
        let mut s = Sketch::default();
        // Collinear.
        let a = s.add_line(p(0.0, 0.0), p(10.0, 1.0));
        let b = s.add_line(p(12.0, 3.0), p(20.0, 2.5));
        s.add_constraint(Constraint::Collinear { a, b }).unwrap();
        // Concentric + Equal circles.
        let c1 = s.add_circle(p(40.0, 0.0), 5.0).unwrap();
        let c2 = s.add_circle(p(43.0, 2.0), 3.0).unwrap();
        s.add_constraint(Constraint::Concentric { a: c1, b: c2 })
            .unwrap();
        s.add_constraint(Constraint::Equal { a: c1, b: c2 })
            .unwrap();
        // Symmetric points about a fixed vertical line x = 0.
        let l0 = s.add_fixed_point(p(0.0, -50.0));
        let l1 = s.add_fixed_point(p(0.0, 50.0));
        let axis = s.add_line_between(l0, l1).unwrap();
        let q1 = s.add_fixed_point(p(3.0, 1.0));
        let q2 = s.add_point(p(-2.0, 2.0));
        s.add_constraint(Constraint::Symmetric {
            a: q1,
            b: q2,
            line: axis,
        })
        .unwrap();
        // Horizontal through two points.
        let h1 = s.add_point(p(60.0, 0.0));
        let h2 = s.add_point(p(70.0, 4.0));
        s.add_constraint(Constraint::HorizontalPoints { a: h1, b: h2 })
            .unwrap();
        assert_eq!(s.solve().unwrap().status, SolveStatus::Solved);

        let (a0, a1) = s.line_geometry(a).unwrap();
        let (b0, b1) = s.line_geometry(b).unwrap();
        let d = a1 - a0;
        assert_relative_eq!(d.perp(&(b0 - a0)), 0.0, epsilon = 1e-8);
        assert_relative_eq!(d.perp(&(b1 - a0)), 0.0, epsilon = 1e-8);
        let ((k1, r1), (k2, r2)) = (
            s.circle_geometry(c1).unwrap(),
            s.circle_geometry(c2).unwrap(),
        );
        assert_relative_eq!(k1, k2, epsilon = 1e-8);
        assert_relative_eq!(r1, r2, epsilon = 1e-8);
        assert_relative_eq!(pos(&s, q2), p(-3.0, 1.0), epsilon = 1e-8);
        assert_relative_eq!(pos(&s, h1).y, pos(&s, h2).y, epsilon = 1e-9);
    }

    #[test]
    fn dimensions_angle_axis_distances_and_driven() {
        use std::f64::consts::PI;
        let mut s = Sketch::default();
        let o = s.add_fixed_point(p(0.0, 0.0));
        let e1 = s.add_point(p(10.0, 0.0));
        let e2 = s.add_point(p(5.0, 8.0));
        let l1 = s.add_line_between(o, e1).unwrap();
        let l2 = s.add_line_between(o, e2).unwrap();
        s.add_constraint(Constraint::Horizontal { line: l1 })
            .unwrap();
        let angle = s
            .add_constraint(Constraint::Angle {
                a: l1,
                b: l2,
                value: PI / 3.0,
                supplement: false,
            })
            .unwrap();
        let hd = s
            .add_constraint(Constraint::HorizontalDistance {
                a: o,
                b: e1,
                value: 25.0,
            })
            .unwrap();
        assert_eq!(s.solve().unwrap().status, SolveStatus::Solved);
        assert_relative_eq!(s.measure(angle).unwrap(), PI / 3.0, epsilon = 1e-9);
        assert_relative_eq!(pos(&s, e1).x, 25.0, epsilon = 1e-9);

        s.set_dimension(angle, PI / 6.0).unwrap();
        assert_eq!(s.solve().unwrap().status, SolveStatus::Solved);
        let dir = pos(&s, e2) - pos(&s, o);
        assert_relative_eq!(dir.y.atan2(dir.x), PI / 6.0, epsilon = 1e-9);
        assert!(s.set_dimension(angle, 4.0).is_err(), "beyond ±180°");

        // A driven dimension only reports; it never moves geometry.
        let len = s
            .add_constraint(Constraint::Distance {
                a: o,
                b: e2,
                value: 1.0,
            })
            .unwrap();
        s.set_driven(len, true).unwrap();
        let before = pos(&s, e2);
        assert_eq!(s.solve().unwrap().status, SolveStatus::Solved);
        assert_eq!(pos(&s, e2), before);
        assert_relative_eq!(
            s.measure(len).unwrap(),
            (before - pos(&s, o)).norm(),
            epsilon = 1e-12
        );
        assert!(s.set_driven(hd, true).is_ok() && s.is_driven(hd));
    }

    #[test]
    fn control_vertex_spline() {
        let mut s = Sketch::default();
        let pts: Vec<_> = [(0.0, 0.0), (10.0, 10.0), (20.0, -5.0), (30.0, 0.0)]
            .iter()
            .map(|&(x, y)| s.add_point(p(x, y)))
            .collect();
        let sp = s.add_spline(pts.clone()).unwrap();
        let curve = s.spline_curve(sp).unwrap();
        assert_eq!(curve.degree(), 3);
        assert_relative_eq!(curve.point_at(0.0).x, 0.0, epsilon = 1e-12);
        assert_relative_eq!(curve.point_at(1.0).x, 30.0, epsilon = 1e-12);
        // Its end can be shared by a line (chaining), and removing an inner
        // control point removes the spline.
        let other = s.add_point(p(40.0, 0.0));
        s.add_line_between(pts[3], other).unwrap();
        s.remove_point(pts[1]).unwrap();
        assert!(s.entity(sp).is_none());
        assert!(s.add_spline(vec![pts[0]]).is_err());
    }

    #[test]
    fn point_on_spline_slides_along_it() {
        let mut s = Sketch::default();
        let cvs: Vec<_> = [(0.0, 0.0), (10.0, 10.0), (20.0, -5.0), (30.0, 0.0)]
            .iter()
            .map(|&(x, y)| s.add_fixed_point(p(x, y)))
            .collect();
        let sp = s.add_spline(cvs.clone()).unwrap();
        let q = s.add_point(p(14.0, 9.0)); // off the curve
        let c = s.on_curve(q, sp).unwrap();
        assert!(matches!(c, Constraint::PointOnSpline { .. }));
        s.add_constraint(c).unwrap();
        let r = s.solve().unwrap();
        assert_eq!((r.status, r.dof, r.redundant), (SolveStatus::Solved, 1, 0));
        let on = |s: &Sketch| {
            let x = s.point(q).unwrap().position.coords;
            let cv: Vec<Vector2> = cvs
                .iter()
                .map(|&c| s.point(c).unwrap().position.coords)
                .collect();
            (constraint::spline_closest(&cv, &[], x).0 - x).norm()
        };
        assert!(on(&s) < 1e-9, "point is on the spline");
        // Pinning its x slides it along the curve.
        s.add_constraint(Constraint::VerticalDistance {
            a: cvs[0],
            b: q,
            value: 3.0,
        })
        .unwrap();
        let r = s.solve().unwrap();
        assert_eq!((r.status, r.dof, r.redundant), (SolveStatus::Solved, 0, 0));
        assert_relative_eq!(s.point(q).unwrap().position.y, 3.0, epsilon = 1e-8);
        assert!(on(&s) < 1e-9);
        // A control point cannot be put on its own spline.
        assert!(s.on_curve(cvs[1], sp).is_none());
    }

    #[test]
    fn relaxed_drag_makes_changed_dimensions_driven() {
        let mut s = Sketch::default();
        let a = s.add_point(p(0.0, 0.0));
        let b = s.add_point(p(10.0, 0.0));
        let c = s.add_point(p(0.0, 5.0));
        let ab = s
            .add_constraint(Constraint::Distance { a, b, value: 10.0 })
            .unwrap();
        let ac = s
            .add_constraint(Constraint::Distance {
                a,
                b: c,
                value: 5.0,
            })
            .unwrap();
        let ids = s.relax_dimensions();
        s.move_point(b, p(14.0, 0.0)).unwrap();
        s.settle_relaxed_dimensions(&ids);
        assert!(s.is_driven(ab), "changed by the drag: driven");
        assert_eq!(s.constraint(ab).and_then(Constraint::value), Some(14.0));
        assert!(!s.is_driven(ac), "untouched: driving again");
    }

    #[test]
    fn dimension_labels_follow_the_geometry_and_round_trip() {
        let mut s = Sketch::default();
        let a = s.add_point(p(0.0, 0.0));
        let b = s.add_point(p(10.0, 0.0));
        let d = s
            .add_constraint(Constraint::Distance { a, b, value: 10.0 })
            .unwrap();
        assert_eq!(s.dimension_label(d), Some(p(5.0, 0.0)), "default: anchor");
        s.set_dimension_label(d, Some(p(5.0, 4.0)));
        s.move_point(b, p(20.0, 0.0)).unwrap();
        assert_eq!(
            s.dimension_label(d),
            Some(p(10.0, 4.0)),
            "moves with the anchor"
        );
        let back: Sketch = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.dimension_label(d), Some(p(10.0, 4.0)));
        s.remove_constraint(d).unwrap();
        assert!(!s.has_dimension_label(d));
    }

    #[test]
    fn angle_quadrant_and_flip_direction() {
        let mut s = Sketch::default();
        let h = s.add_line(p(0.0, 0.0), p(10.0, 0.0));
        let d = s.add_line(p(0.0, 0.0), p(5.0, 5.0)); // 45°
        let deg = |v: Real| v.to_degrees();
        let (m, sup) = s.angle_for_placement(h, d, p(5.0, 1.0)).unwrap();
        assert!((deg(m) - 45.0).abs() < 1e-9 && !sup);
        let (m2, sup2) = s.angle_for_placement(h, d, p(-3.0, 2.0)).unwrap();
        assert!((deg(m2).abs() - 135.0).abs() < 1e-9 && sup2);
        // The supplementary dimension holds the geometry as it is and drives it.
        let id = s
            .add_constraint(Constraint::Angle {
                a: h,
                b: d,
                value: m2,
                supplement: true,
            })
            .unwrap();
        assert!((s.measure(id).unwrap() - m2).abs() < 1e-9, "zero residual");
        s.set_dimension(id, m2.signum() * 120f64.to_radians())
            .unwrap();
        assert_eq!(s.solve().unwrap().status, SolveStatus::Solved);
        // Both lines are free: check their angle to each other.
        let dir = |l| {
            let (q0, q1) = s.line_geometry(l).unwrap();
            (q1 - q0).y.atan2((q1 - q0).x)
        };
        let rel = deg(constraint::wrap_angle(dir(d) - dir(h)));
        assert!((rel - 60.0).abs() < 1e-6, "60° between them now: {rel}");
        s.remove_constraint(id).unwrap();
        // Flip of a horizontal distance moves the point to the other side.
        let a = s.add_fixed_point(p(20.0, 0.0));
        let b = s.add_point(p(25.0, 3.0));
        let hd = s
            .add_constraint(Constraint::HorizontalDistance { a, b, value: 5.0 })
            .unwrap();
        s.solve().unwrap();
        s.flip_dimension(hd).unwrap();
        assert!((s.point(b).unwrap().position.x - 15.0).abs() < 1e-8);
        // Aligned distances cannot be flipped; nothing changes.
        let ad = s
            .add_constraint(Constraint::Distance { a, b, value: 5.0 })
            .unwrap();
        let _ = s.remove_constraint(hd);
        let json = serde_json::to_string(&s).unwrap();
        assert!(s.flip_dimension(ad).is_err());
        assert_eq!(serde_json::to_string(&s).unwrap(), json);
    }

    #[test]
    fn dof_analysis_per_geometry() {
        let mut s = Sketch::default();
        let a = s.add_fixed_point(p(0.0, 0.0));
        let b = s.add_point(p(10.0, 0.0));
        let l = s.add_line_between(a, b).unwrap();
        // Free end: 2D; line rotates.
        let d = s.dof_analysis();
        assert_eq!(d.points[&b], PointFreedom::Free);
        assert!(d.rotating.contains(&l));
        // Horizontal: the end slides along X, no rotation.
        s.add_constraint(Constraint::Horizontal { line: l })
            .unwrap();
        let d = s.dof_analysis();
        match d.points[&b] {
            PointFreedom::Along(v) => assert_relative_eq!(v.x.abs(), 1.0, epsilon = 1e-6),
            other => panic!("{other:?}"),
        }
        assert!(!d.rotating.contains(&l) && !d.fully_constrained.contains(&l));
        // Length fixes it completely.
        s.add_constraint(Constraint::Distance { a, b, value: 10.0 })
            .unwrap();
        let d = s.dof_analysis();
        assert_eq!(d.points[&b], PointFreedom::Fixed);
        assert!(d.fully_constrained.contains(&l));
        // A circle on a fixed centre still has a free radius.
        let c = s.add_circle_at(a, 3.0).unwrap();
        let d = s.dof_analysis();
        assert!(d.free_radius.contains(&c) && !d.fully_constrained.contains(&c));
    }

    #[test]
    fn removing_a_point_cascades() {
        let mut s = Sketch::default();
        let (c, l) = rough_rectangle(&mut s);
        s.add_constraint(Constraint::Distance {
            a: c[0],
            b: c[2],
            value: 5.0,
        })
        .unwrap();
        // c[1] is shared by bottom and right lines.
        s.remove_point(c[1]).unwrap();
        assert!(s.entity(l[0]).is_none() && s.entity(l[1]).is_none());
        assert!(s.entity(l[2]).is_some() && s.entity(l[3]).is_some());
        // Horizontal(bottom) and Vertical(right) went with their lines.
        assert_eq!(s.constraints().count(), 3);
        s.remove_entity(l[2]).unwrap();
        assert_eq!(s.constraints().count(), 2);
    }

    #[test]
    fn coincident_joins_points() {
        let mut s = Sketch::default();
        let a = s.add_line(p(0.0, 0.0), p(5.0, 0.0));
        let b = s.add_line(p(5.2, 0.1), p(5.0, 5.0));
        let (_, a_end) = s.line_points(a).unwrap();
        let (b_start, _) = s.line_points(b).unwrap();
        s.add_constraint(Constraint::Coincident {
            a: a_end,
            b: b_start,
        })
        .unwrap();

        let r = s.solve().unwrap();
        assert_eq!(r.status, SolveStatus::Solved);
        assert_relative_eq!(pos(&s, a_end), pos(&s, b_start), epsilon = 1e-9);
    }

    #[test]
    fn rectangle_dof_counting() {
        let mut s = Sketch::default();
        let (c, l) = rough_rectangle(&mut s);

        // 4 points × 2 = 8 DOF, 4 H/V equations ⇒ x, y, width, height remain.
        let r = s.solve().unwrap();
        assert_eq!((r.status, r.dof, r.redundant), (SolveStatus::Solved, 4, 0));

        s.set_fixed(c[0], true).unwrap();
        assert_eq!(s.solve().unwrap().dof, 2);

        s.add_constraint(Constraint::Distance {
            a: c[0],
            b: c[1],
            value: 20.0,
        })
        .unwrap();
        s.add_constraint(Constraint::Distance {
            a: c[1],
            b: c[2],
            value: 8.0,
        })
        .unwrap();
        let r = s.solve().unwrap();
        assert!(r.is_fully_constrained(), "{r:?}");

        let origin = pos(&s, c[0]);
        assert_relative_eq!(
            pos(&s, c[2]),
            origin + Vector2::new(20.0, 8.0),
            epsilon = 1e-9
        );
        // Redundant but consistent: top ∥ bottom is already implied by H/H.
        s.add_constraint(Constraint::Parallel { a: l[0], b: l[2] })
            .unwrap();
        let r = s.solve().unwrap();
        assert_eq!((r.status, r.redundant), (SolveStatus::Solved, 1));
    }

    #[test]
    fn parallel_and_perpendicular() {
        let mut s = Sketch::default();
        let a = s.add_line(p(0.0, 0.0), p(10.0, 1.0));
        let b = s.add_line(p(0.0, 3.0), p(10.0, 5.0));
        let c = s.add_line(p(2.0, -2.0), p(3.0, 8.0));
        s.add_constraint(Constraint::Parallel { a, b }).unwrap();
        s.add_constraint(Constraint::Perpendicular { a, b: c })
            .unwrap();
        assert_eq!(s.solve().unwrap().status, SolveStatus::Solved);

        let dir = |l| {
            let (u, v) = s.line_points(l).unwrap();
            (pos(&s, v) - pos(&s, u)).normalize()
        };
        assert_relative_eq!(dir(a).perp(&dir(b)), 0.0, epsilon = 1e-9);
        assert_relative_eq!(dir(a).dot(&dir(c)), 0.0, epsilon = 1e-9);
    }

    #[test]
    fn line_circle_tangent() {
        let mut s = Sketch::default();
        let line = s.add_line(p(-10.0, 4.0), p(10.0, 4.5));
        let circle = s.add_circle(p(0.0, 0.0), 3.0).unwrap();
        s.add_tangent(line, circle).unwrap();
        assert_eq!(s.solve().unwrap().status, SolveStatus::Solved);

        let (a, b) = s.line_points(line).unwrap();
        let (c, r) = s.circle_now(circle).unwrap();
        let d = pos(&s, b) - pos(&s, a);
        let dist = d.perp(&(c - pos(&s, a))).abs() / d.norm();
        assert_relative_eq!(dist, r, epsilon = 1e-9);
    }

    #[test]
    fn under_constrained_geometry_moves_minimally() {
        // A horizontal line tangent to a fixed circle: its endpoints may slide
        // freely along the line, but the solver must not exploit that freedom.
        let mut s = Sketch::default();
        let center = s.add_fixed_point(p(0.0, 0.0));
        let big = s.add_circle_at(center, 14.0).unwrap();
        s.add_constraint(Constraint::Radius {
            circle: big,
            value: 15.0,
        })
        .unwrap();
        let line = s.add_line(p(-25.0, 17.0), p(25.0, 18.5));
        s.add_constraint(Constraint::Horizontal { line }).unwrap();
        s.add_tangent(line, big).unwrap();
        let small = s.add_circle(p(22.0, 9.0), 5.0).unwrap();
        s.add_tangent(big, small).unwrap();
        s.add_tangent(line, small).unwrap();

        let before: Vec<_> = s.points().map(|(_, q)| q.position).collect();
        assert_eq!(s.solve().unwrap().status, SolveStatus::Solved);
        for (b, (_, q)) in before.iter().zip(s.points()) {
            assert!(
                (q.position - b).norm() < 5.0,
                "point moved from {b} to {}",
                q.position
            );
        }
    }

    #[test]
    fn circle_circle_tangent_picks_contact_kind() {
        let mut s = Sketch::default();
        let big = s.add_circle(p(0.0, 0.0), 10.0).unwrap();
        let outer = s.add_circle(p(13.5, 0.0), 3.0).unwrap(); // nearly external
        let inner = s.add_circle(p(0.0, 6.5), 3.0).unwrap(); // nearly internal
        let c_ext = s.add_tangent(big, outer).unwrap();
        let c_int = s.add_tangent(big, inner).unwrap();
        assert!(matches!(
            s.constraint(c_ext),
            Some(Constraint::Tangent {
                internal: false,
                ..
            })
        ));
        assert!(matches!(
            s.constraint(c_int),
            Some(Constraint::Tangent { internal: true, .. })
        ));
        assert_eq!(s.solve().unwrap().status, SolveStatus::Solved);

        let (c0, r0) = s.circle_now(big).unwrap();
        let (c1, r1) = s.circle_now(outer).unwrap();
        let (c2, r2) = s.circle_now(inner).unwrap();
        assert_relative_eq!((c1 - c0).norm(), r0 + r1, epsilon = 1e-9);
        assert_relative_eq!((c2 - c0).norm(), r0 - r2, epsilon = 1e-9);
    }

    #[test]
    fn arc_endpoints_stay_on_one_circle() {
        let mut s = Sketch::default();
        let c = s.add_fixed_point(p(0.0, 0.0));
        let a = s.add_point(p(5.0, 0.0));
        let b = s.add_point(p(0.0, 4.0));
        let arc = s.add_arc(c, a, b).unwrap();
        s.add_constraint(Constraint::Radius {
            circle: arc,
            value: 6.0,
        })
        .unwrap();
        assert_eq!(s.solve().unwrap().status, SolveStatus::Solved);
        assert_relative_eq!(pos(&s, a).coords.norm(), 6.0, epsilon = 1e-9);
        assert_relative_eq!(pos(&s, b).coords.norm(), 6.0, epsilon = 1e-9);
    }

    #[test]
    fn inconsistent_constraints_fail_and_leave_sketch_unchanged() {
        let mut s = Sketch::default();
        let a = s.add_fixed_point(p(0.0, 0.0));
        let b = s.add_point(p(5.0, 0.0));
        s.add_constraint(Constraint::Distance { a, b, value: 10.0 })
            .unwrap();
        s.add_constraint(Constraint::Distance { a, b, value: 20.0 })
            .unwrap();
        let before = pos(&s, b);
        let r = s.solve().unwrap();
        assert_eq!(r.status, SolveStatus::Failed);
        assert_eq!(pos(&s, b), before);
    }

    #[test]
    fn invalid_constraints_are_rejected() {
        let mut s = Sketch::default();
        let circle = s.add_circle(p(0.0, 0.0), 1.0).unwrap();
        let line = s.add_line(p(0.0, 0.0), p(1.0, 0.0));
        assert_eq!(
            s.add_constraint(Constraint::Horizontal { line: circle }),
            Err(SketchError::WrongEntityKind("Horizontal"))
        );
        assert!(
            s.add_constraint(Constraint::Tangent {
                a: line,
                b: line,
                internal: false
            })
            .is_err()
        );
        assert!(
            s.add_constraint(Constraint::Radius {
                circle,
                value: -1.0
            })
            .is_err()
        );
    }

    #[test]
    fn sketch_roundtrips_through_json_and_maps_to_world() {
        let mut s = Sketch::new(
            Plane::new(
                Point3::new(0.0, 0.0, 5.0),
                crate::kernel::Vector3::z(),
                crate::kernel::Vector3::x(),
            )
            .unwrap(),
        );
        let (c, _) = rough_rectangle(&mut s);
        s.solve().unwrap();
        let back: Sketch = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.point(c[2]), s.point(c[2]));
        assert_relative_eq!(s.to_world(&pos(&s, c[0])).z, 5.0);
    }
}
