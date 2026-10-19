//! # The 3DCadCraft geometric kernel
//!
//! A clean-room, lightweight **B-Rep (Boundary Representation)** kernel.
//!
//! A solid is described by its boundary: a set of faces, each bounded by
//! loops of edges, each edge bounded by two vertices. The kernel keeps two
//! strictly separated layers:
//!
//! | Layer        | Answers                     | Types                                              |
//! |--------------|-----------------------------|----------------------------------------------------|
//! | **Geometry** | *Where* is it? (shape)      | [`Point3`], [`Vector3`], [`Line`], [`Circle`], [`BSplineCurve`], [`Plane`], [`Cylinder`] |
//! | **Topology** | *How* is it connected?      | [`Vertex`], [`Edge`], [`Coedge`], [`Loop`], [`Face`], [`Shell`], [`Solid`] |
//!
//! Topology references geometry through IDs, so one surface may be shared by
//! several faces and one curve by several edges (e.g. after a split).
//!
//! ## Storage model
//!
//! All entities live in a single [`BRep`] container, inside generational
//! arenas ([`slotmap::SlotMap`]). Entities refer to each other through small,
//! `Copy`, strongly-typed keys ([`VertexId`], [`EdgeId`], ...). This gives us:
//!
//! - no `Rc<RefCell<..>>` cycles and no `unsafe` pointer graphs;
//! - stale-ID detection (a deleted-then-reused slot has a new generation);
//! - trivial `Clone` of a whole model (cheap snapshots for undo/redo) and
//!   `serde` (de)serialisation for files and the JSON-RPC/MCP API.
//!
//! ## Conventions
//!
//! - Units: **millimetres** and **radians**. Scalars are [`Real`] (`f64`).
//! - Every comparison of geometric quantities goes through [`Tolerance`],
//!   never through `==` on floats.
//! - Orientation: a face's loops run **counter-clockwise when viewed from
//!   outside the solid** (right-hand rule gives the outward normal). Inner
//!   loops (holes) therefore run clockwise from outside.

pub mod csg;
pub mod exact;
mod extrude;
mod facet;
mod mesh;
mod query;
mod revolve;

pub use extrude::{ExtrudeLevel, ProfileSegment, Segment2};
pub use facet::{BoolOp, FaceTag};
pub use mesh::{SolidMesh, TessellationOptions, TriangleMesh, triangulate_with_holes};
pub use revolve::RevolveAxis;

use std::collections::HashMap;

use nalgebra as na;
use serde::{Deserialize, Serialize};
use slotmap::{SlotMap, new_key_type};
use thiserror::Error;

// ============================================================================
// Scalars, points, vectors, transforms
// ============================================================================

/// The scalar type used throughout the kernel.
///
/// `f64` is non-negotiable for CAD: `f32` only has ~7 significant digits, so a
/// 1 m part would already lose micron accuracy.
pub type Real = f64;

/// A position in 3D model space (millimetres).
///
/// `nalgebra` distinguishes points from vectors at the type level: a point is
/// affected by translation, a vector is not. Keeping that distinction prevents
/// a whole class of transformation bugs.
pub type Point3 = na::Point3<Real>;

/// A displacement / direction in 3D (not necessarily unit length).
pub type Vector3 = na::Vector3<Real>;

/// A vector that is guaranteed (by construction) to have unit length.
pub type UnitVector3 = na::Unit<Vector3>;

/// A rigid transformation (rotation + translation) of 3D space.
///
/// Rigid motions preserve lengths and angles, so every geometric type can be
/// transformed exactly. Non-uniform scaling/shearing changes the *type* of some
/// geometry (a circle becomes an ellipse) and will be handled separately.
pub type Transform3 = na::Isometry3<Real>;

/// Geometric types that can be moved rigidly through space.
pub trait Transformable {
    /// Applies `t` in place.
    fn transform(&mut self, t: &Transform3);

    /// Returns a transformed copy.
    fn transformed(&self, t: &Transform3) -> Self
    where
        Self: Clone,
    {
        let mut copy = self.clone();
        copy.transform(t);
        copy
    }
}

impl Transformable for Point3 {
    fn transform(&mut self, t: &Transform3) {
        *self = t.transform_point(self);
    }
}

/// Rotates a unit vector. Rotation preserves length, so we only renormalise
/// to scrub accumulated floating-point drift.
fn rotate_unit(t: &Transform3, v: &UnitVector3) -> UnitVector3 {
    UnitVector3::new_normalize(t.rotation * v.into_inner())
}

// ============================================================================
// Tolerances & errors
// ============================================================================

/// Modelling tolerances.
///
/// Floating-point geometry is never exact: two points that "should" coincide
/// after an intersection will differ in the last bits. All equality tests in
/// the kernel are therefore made *within a tolerance*.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tolerance {
    /// Two points closer than this are considered the same point (mm).
    pub linear: Real,
    /// Two directions whose angle differs by less than this are considered
    /// parallel (radians).
    pub angular: Real,
}

impl Default for Tolerance {
    /// 1e-6 mm (one nanometre) and 1e-9 rad: tight enough for precision
    /// engineering, loose enough to absorb `f64` round-off on parts up to
    /// several metres.
    fn default() -> Self {
        Self {
            linear: 1e-6,
            angular: 1e-9,
        }
    }
}

impl Tolerance {
    /// Do `a` and `b` coincide within the linear tolerance?
    pub fn points_coincide(&self, a: &Point3, b: &Point3) -> bool {
        na::distance_squared(a, b) <= self.linear * self.linear
    }

    /// Is `v` too short to define a direction?
    pub fn is_zero_vector(&self, v: &Vector3) -> bool {
        v.norm_squared() <= self.linear * self.linear
    }

    /// Are `a` and `b` parallel or anti-parallel within the angular tolerance?
    pub fn are_parallel(&self, a: &UnitVector3, b: &UnitVector3) -> bool {
        // |a × b| = sin(θ); for small θ, sin(θ) ≈ θ.
        a.cross(b).norm() <= self.angular
    }

    /// Are `a` and `b` perpendicular within the angular tolerance?
    pub fn are_perpendicular(&self, a: &UnitVector3, b: &UnitVector3) -> bool {
        // a · b = cos(θ) = sin(π/2 − θ) ≈ (π/2 − θ) near 90°.
        a.dot(b).abs() <= self.angular
    }
}

/// Everything that can go wrong inside the kernel.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum KernelError {
    /// The input would produce geometry with no extent (zero-length direction,
    /// zero radius, coincident line endpoints, ...).
    #[error("degenerate geometry: {0}")]
    Degenerate(&'static str),

    /// A B-spline definition violates the mathematical requirements.
    #[error("invalid B-spline: {0}")]
    InvalidBSpline(String),

    /// An interval with `min > max` or non-finite bounds.
    #[error("invalid interval [{min}, {max}]")]
    InvalidInterval {
        /// Lower bound supplied.
        min: Real,
        /// Upper bound supplied.
        max: Real,
    },

    /// An ID that does not (or no longer) refers to a live entity.
    #[error("dangling reference to {0}")]
    DanglingReference(&'static str),

    /// Topology is inconsistent (open loop, non-manifold edge, ...).
    #[error("invalid topology: {0}")]
    InvalidTopology(String),

    /// The operation is not implemented for this kind of geometry yet.
    #[error("unsupported: {0}")]
    Unsupported(&'static str),
}

/// Convenience alias for kernel results.
pub type KernelResult<T> = Result<T, KernelError>;

// ============================================================================
// Parameter intervals
// ============================================================================

/// A closed parameter interval `[min, max]` on a curve.
///
/// Curves are parameterised by a single real `t`; an edge uses the part of
/// its curve between `min` and `max`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Interval {
    /// Lower bound (inclusive).
    pub min: Real,
    /// Upper bound (inclusive).
    pub max: Real,
}

impl Interval {
    /// Creates `[min, max]`; fails if the bounds are not finite or out of order.
    pub fn new(min: Real, max: Real) -> KernelResult<Self> {
        if !(min.is_finite() && max.is_finite() && min <= max) {
            return Err(KernelError::InvalidInterval { min, max });
        }
        Ok(Self { min, max })
    }

    /// `max - min`.
    pub fn length(&self) -> Real {
        self.max - self.min
    }

    /// Is `t` inside the interval, allowing `eps` slack at both ends?
    pub fn contains(&self, t: Real, eps: Real) -> bool {
        t >= self.min - eps && t <= self.max + eps
    }

    /// Clamps `t` into the interval.
    pub fn clamp(&self, t: Real) -> Real {
        t.clamp(self.min, self.max)
    }

    /// Maps the normalised parameter `s ∈ [0, 1]` onto the interval.
    pub fn lerp(&self, s: Real) -> Real {
        self.min + s * self.length()
    }
}

// ============================================================================
// Curves
// ============================================================================

/// An unbounded straight line `P(t) = origin + t · direction`.
///
/// Because `direction` is unit length, `t` is the signed arc-length distance
/// from `origin`, which makes edge parameter ranges directly measurable in mm.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Line {
    /// A point on the line (the `t = 0` point).
    pub origin: Point3,
    /// Unit direction of increasing `t`.
    pub direction: UnitVector3,
}

impl Line {
    /// Creates a line through `origin` along `direction` (normalised here).
    pub fn new(origin: Point3, direction: Vector3) -> KernelResult<Self> {
        let direction = UnitVector3::try_new(direction, Real::EPSILON)
            .ok_or(KernelError::Degenerate("line direction has zero length"))?;
        Ok(Self { origin, direction })
    }

    /// Creates the line through `a` and `b`, with `a` at `t = 0` and `b` at
    /// `t = |b - a|`.
    pub fn through_points(a: Point3, b: Point3, tol: &Tolerance) -> KernelResult<Self> {
        let d = b - a;
        if tol.is_zero_vector(&d) {
            return Err(KernelError::Degenerate("line endpoints coincide"));
        }
        Self::new(a, d)
    }

    /// Evaluates the line at parameter `t`.
    pub fn point_at(&self, t: Real) -> Point3 {
        self.origin + self.direction.into_inner() * t
    }

    /// The parameter of the orthogonal projection of `p` onto the line.
    pub fn parameter_of(&self, p: &Point3) -> Real {
        (p - self.origin).dot(&self.direction)
    }

    /// The point on the line closest to `p`.
    pub fn closest_point(&self, p: &Point3) -> Point3 {
        self.point_at(self.parameter_of(p))
    }

    /// Shortest distance from `p` to the line.
    pub fn distance_to(&self, p: &Point3) -> Real {
        na::distance(p, &self.closest_point(p))
    }
}

impl Transformable for Line {
    fn transform(&mut self, t: &Transform3) {
        self.origin = t.transform_point(&self.origin);
        self.direction = rotate_unit(t, &self.direction);
    }
}

/// A full circle in 3D, parameterised by angle:
///
/// `P(θ) = center + r·(cos θ · x_axis + sin θ · y_axis)`, with
/// `y_axis = normal × x_axis`.
///
/// Arcs are edges that use a sub-interval of `θ`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Circle {
    /// Centre point.
    pub center: Point3,
    /// Axis of the circle; the circle runs counter-clockwise around it.
    pub normal: UnitVector3,
    /// Direction from the centre to the `θ = 0` point (⟂ `normal`).
    pub x_axis: UnitVector3,
    /// Radius (> 0).
    pub radius: Real,
}

impl Circle {
    /// Creates a circle. `x_axis` is projected onto the plane of the circle,
    /// so it only needs to be roughly perpendicular to `normal`.
    pub fn new(
        center: Point3,
        normal: Vector3,
        x_axis: Vector3,
        radius: Real,
    ) -> KernelResult<Self> {
        if !(radius.is_finite() && radius > 0.0) {
            return Err(KernelError::Degenerate("circle radius must be positive"));
        }
        let (normal, x_axis) = orthonormal_frame(normal, x_axis)?;
        Ok(Self {
            center,
            normal,
            x_axis,
            radius,
        })
    }

    /// The second in-plane axis, `normal × x_axis`.
    pub fn y_axis(&self) -> UnitVector3 {
        UnitVector3::new_unchecked(self.normal.cross(&self.x_axis))
    }

    /// Evaluates the circle at angle `theta` (radians).
    pub fn point_at(&self, theta: Real) -> Point3 {
        let (s, c) = theta.sin_cos();
        self.center + self.radius * (c * self.x_axis.into_inner() + s * self.y_axis().into_inner())
    }

    /// First derivative `dP/dθ` (tangent, length = radius).
    pub fn derivative_at(&self, theta: Real) -> Vector3 {
        let (s, c) = theta.sin_cos();
        self.radius * (-s * self.x_axis.into_inner() + c * self.y_axis().into_inner())
    }
}

impl Transformable for Circle {
    fn transform(&mut self, t: &Transform3) {
        self.center = t.transform_point(&self.center);
        self.normal = rotate_unit(t, &self.normal);
        self.x_axis = rotate_unit(t, &self.x_axis);
    }
}

/// A (possibly rational) B-spline curve, i.e. a **NURBS** curve.
///
/// Definition, for `n` control points `Pᵢ`, weights `wᵢ`, degree `p` and knot
/// vector `U = [u₀ … u_{n+p}]`:
///
/// ```text
///            Σ Nᵢ,ₚ(t) · wᵢ · Pᵢ
///   C(t) = ───────────────────────        t ∈ [u_p, u_n]
///             Σ Nᵢ,ₚ(t) · wᵢ
/// ```
///
/// where `Nᵢ,ₚ` are the Cox–de Boor basis functions. With all weights equal
/// (`weights == None`) this reduces to a polynomial B-spline. Rational weights
/// let us represent conics (circles, ellipses) exactly, which is why STEP/IGES
/// use NURBS as the universal free-form curve.
///
/// Fields are private so that every instance is guaranteed valid; build one
/// with [`BSplineCurve::new`] or [`BSplineCurve::clamped_uniform`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BSplineCurve {
    degree: usize,
    control_points: Vec<Point3>,
    knots: Vec<Real>,
    /// `None` ⇒ non-rational (all weights 1).
    weights: Option<Vec<Real>>,
}

impl BSplineCurve {
    /// Creates a B-spline / NURBS curve, validating all invariants:
    ///
    /// - `degree ≥ 1` and at least `degree + 1` control points;
    /// - `knots.len() == control_points.len() + degree + 1`;
    /// - knots finite and non-decreasing, with a non-empty domain
    ///   `[u_p, u_n]`;
    /// - if given, one strictly positive finite weight per control point.
    pub fn new(
        degree: usize,
        control_points: Vec<Point3>,
        knots: Vec<Real>,
        weights: Option<Vec<Real>>,
    ) -> KernelResult<Self> {
        let bad = |msg: String| Err(KernelError::InvalidBSpline(msg));
        let n = control_points.len();

        if degree == 0 {
            return bad("degree must be at least 1".into());
        }
        if n < degree + 1 {
            return bad(format!(
                "degree {degree} needs ≥ {} control points, got {n}",
                degree + 1
            ));
        }
        if knots.len() != n + degree + 1 {
            return bad(format!(
                "expected {} knots, got {}",
                n + degree + 1,
                knots.len()
            ));
        }
        if knots.iter().any(|k| !k.is_finite()) {
            return bad("knots must be finite".into());
        }
        if knots.windows(2).any(|w| w[0] > w[1]) {
            return bad("knot vector must be non-decreasing".into());
        }
        if knots[degree] >= knots[n] {
            return bad("parameter domain [u_p, u_n] is empty".into());
        }
        if let Some(w) = &weights {
            if w.len() != n {
                return bad(format!("expected {n} weights, got {}", w.len()));
            }
            if w.iter().any(|&wi| !(wi.is_finite() && wi > 0.0)) {
                return bad("weights must be finite and strictly positive".into());
            }
        }
        Ok(Self {
            degree,
            control_points,
            knots,
            weights,
        })
    }

    /// A non-rational curve with a *clamped uniform* knot vector on `[0, 1]`.
    ///
    /// "Clamped" (end knots repeated `degree + 1` times) makes the curve start
    /// at the first control point and end at the last, tangent to the control
    /// polygon there, which is what users expect from a sketch spline.
    pub fn clamped_uniform(degree: usize, control_points: Vec<Point3>) -> KernelResult<Self> {
        let n = control_points.len();
        if degree == 0 || n < degree + 1 {
            // Defer to `new` for a precise message.
            return Self::new(degree, control_points, Vec::new(), None);
        }
        let interior = n - degree; // number of spans
        let knots = (0..n + degree + 1)
            .map(|i| {
                if i <= degree {
                    0.0
                } else if i >= n {
                    1.0
                } else {
                    (i - degree) as Real / interior as Real
                }
            })
            .collect();
        Self::new(degree, control_points, knots, None)
    }

    /// A sketch spline: clamped with the given full knot vector, or clamped
    /// uniform (degree `min(3, n − 1)`) when `knots` is empty.
    pub fn sketch_spline(control_points: Vec<Point3>, knots: &[Real]) -> KernelResult<Self> {
        let n = control_points.len();
        if knots.is_empty() || n == 0 {
            return Self::clamped_uniform((n.max(1) - 1).min(3), control_points);
        }
        let degree = knots.len().saturating_sub(n + 1);
        Self::new(degree, control_points, knots.to_vec(), None)
    }

    /// Polynomial degree `p`.
    pub fn degree(&self) -> usize {
        self.degree
    }

    /// Control points `Pᵢ`.
    pub fn control_points(&self) -> &[Point3] {
        &self.control_points
    }

    /// Knot vector `U`.
    pub fn knots(&self) -> &[Real] {
        &self.knots
    }

    /// Weights, if the curve is rational.
    pub fn weights(&self) -> Option<&[Real]> {
        self.weights.as_deref()
    }

    /// Is this a rational (NURBS) curve?
    pub fn is_rational(&self) -> bool {
        self.weights.is_some()
    }

    /// The valid parameter range `[u_p, u_n]`.
    pub fn domain(&self) -> Interval {
        Interval {
            min: self.knots[self.degree],
            max: self.knots[self.control_points.len()],
        }
    }

    /// Control points lifted to homogeneous space: `(w·x, w·y, w·z, w)`.
    ///
    /// In homogeneous space a NURBS curve is an ordinary polynomial B-spline,
    /// so one evaluation algorithm handles both cases.
    fn homogeneous_control_points(&self) -> Vec<na::Vector4<Real>> {
        self.control_points
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let w = self.weights.as_ref().map_or(1.0, |w| w[i]);
                na::Vector4::new(p.x * w, p.y * w, p.z * w, w)
            })
            .collect()
    }

    /// Evaluates the curve at `t` (clamped into [`Self::domain`]).
    pub fn point_at(&self, t: Real) -> Point3 {
        let t = self.domain().clamp(t);
        let h = de_boor(
            self.degree,
            &self.knots,
            &self.homogeneous_control_points(),
            t,
        );
        Point3::from(h.xyz() / h.w)
    }

    /// First derivative `C'(t)` (clamped into [`Self::domain`]).
    ///
    /// Computed exactly: differentiate the homogeneous curve `A(t) = (w·C, w)`
    /// via its hodograph, then apply the quotient rule
    /// `C' = (A'_xyz − w'·C) / w`.
    pub fn derivative_at(&self, t: Real) -> Vector3 {
        let t = self.domain().clamp(t);
        let p = self.degree;
        let ctrl = self.homogeneous_control_points();

        // Hodograph: degree p−1, control points Qᵢ = p·(Pᵢ₊₁ − Pᵢ)/(u_{i+p+1} − u_{i+1}),
        // knot vector U with the first and last knot removed.
        let deriv_ctrl: Vec<_> = (0..ctrl.len() - 1)
            .map(|i| {
                let denom = self.knots[i + p + 1] - self.knots[i + 1];
                if denom > 0.0 {
                    (ctrl[i + 1] - ctrl[i]) * (p as Real / denom)
                } else {
                    na::Vector4::zeros()
                }
            })
            .collect();
        let deriv_knots = &self.knots[1..self.knots.len() - 1];

        let a = de_boor(p, &self.knots, &ctrl, t);
        let da = de_boor(p - 1, deriv_knots, &deriv_ctrl, t);
        let c = a.xyz() / a.w;
        (da.xyz() - c * da.w) / a.w
    }
}

impl Transformable for BSplineCurve {
    /// B-splines are affinely invariant: transforming the control points
    /// transforms the curve.
    fn transform(&mut self, t: &Transform3) {
        for p in &mut self.control_points {
            *p = t.transform_point(p);
        }
    }
}

/// Finds the knot span index `k` with `U[k] ≤ t < U[k+1]` inside the domain
/// `[U[p], U[n]]` (Piegl & Tiller, *The NURBS Book*, algorithm A2.1).
///
/// `n` is the number of control points. At the right end of the domain the
/// last non-empty span is returned so the curve is closed at `t = U[n]`.
fn find_span(degree: usize, knots: &[Real], n: usize, t: Real) -> usize {
    if t >= knots[n] {
        // Walk back over repeated end knots to the last non-empty span.
        let mut k = n - 1;
        while k > degree && knots[k] >= knots[n] {
            k -= 1;
        }
        return k;
    }
    if t <= knots[degree] {
        return degree;
    }
    // Binary search: invariant knots[lo] ≤ t < knots[hi].
    let (mut lo, mut hi) = (degree, n);
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if t < knots[mid] {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    lo
}

/// De Boor's algorithm: evaluates a polynomial B-spline with (homogeneous)
/// control points `ctrl` at `t`.
///
/// It repeatedly blends neighbouring control points (`p` rounds of linear
/// interpolation), which is numerically stable and needs no explicit basis
/// functions.
fn de_boor(
    degree: usize,
    knots: &[Real],
    ctrl: &[na::Vector4<Real>],
    t: Real,
) -> na::Vector4<Real> {
    let k = find_span(degree, knots, ctrl.len(), t);
    // The p+1 control points that influence span k.
    let mut d: Vec<_> = (0..=degree).map(|j| ctrl[j + k - degree]).collect();

    for r in 1..=degree {
        for j in (r..=degree).rev() {
            let i = j + k - degree;
            let denom = knots[i + degree + 1 - r] - knots[i];
            let alpha = if denom > 0.0 {
                (t - knots[i]) / denom
            } else {
                0.0
            };
            d[j] = d[j - 1] * (1.0 - alpha) + d[j] * alpha;
        }
    }
    d[degree]
}

/// Any 3D curve the kernel understands.
///
/// An enum (rather than `dyn Trait`) keeps the set closed and exhaustively
/// matchable — every algorithm (intersection, offset, export) must decide
/// what to do for every curve kind — and makes the type trivially
/// serialisable.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Curve {
    /// Straight line.
    Line(Line),
    /// Circle (arcs are edges on a circle).
    Circle(Circle),
    /// Free-form NURBS curve.
    BSpline(BSplineCurve),
}

impl Curve {
    /// Evaluates the curve at parameter `t`.
    pub fn point_at(&self, t: Real) -> Point3 {
        match self {
            Curve::Line(c) => c.point_at(t),
            Curve::Circle(c) => c.point_at(t),
            Curve::BSpline(c) => c.point_at(t),
        }
    }

    /// First derivative with respect to `t` (tangent direction × speed).
    pub fn derivative_at(&self, t: Real) -> Vector3 {
        match self {
            Curve::Line(c) => c.direction.into_inner(),
            Curve::Circle(c) => c.derivative_at(t),
            Curve::BSpline(c) => c.derivative_at(t),
        }
    }

    /// Unit tangent at `t`, or `None` where the curve is singular
    /// (zero derivative, e.g. a cusp).
    pub fn tangent_at(&self, t: Real) -> Option<UnitVector3> {
        UnitVector3::try_new(self.derivative_at(t), Real::EPSILON)
    }

    /// The natural parameter domain, or `None` if the curve is unbounded.
    pub fn domain(&self) -> Option<Interval> {
        match self {
            Curve::Line(_) => None,
            Curve::Circle(_) => Some(Interval {
                min: 0.0,
                max: std::f64::consts::TAU,
            }),
            Curve::BSpline(c) => Some(c.domain()),
        }
    }
}

impl Transformable for Curve {
    fn transform(&mut self, t: &Transform3) {
        match self {
            Curve::Line(c) => c.transform(t),
            Curve::Circle(c) => c.transform(t),
            Curve::BSpline(c) => c.transform(t),
        }
    }
}

// ============================================================================
// Surfaces
// ============================================================================

/// Builds a right-handed orthonormal frame `(normal, x_axis)` from a normal and
/// a hint for the x-axis (Gram–Schmidt). If the hint is (nearly) parallel to
/// the normal, an arbitrary perpendicular axis is chosen.
fn orthonormal_frame(normal: Vector3, x_hint: Vector3) -> KernelResult<(UnitVector3, UnitVector3)> {
    let normal = UnitVector3::try_new(normal, Real::EPSILON)
        .ok_or(KernelError::Degenerate("normal has zero length"))?;
    let projected = x_hint - normal.into_inner() * x_hint.dot(&normal);
    let x_axis =
        UnitVector3::try_new(projected, 1e-12).unwrap_or_else(|| any_perpendicular(&normal));
    Ok((normal, x_axis))
}

/// Some unit vector perpendicular to `n` (stable choice: cross with the world
/// axis least aligned with `n`).
pub fn any_perpendicular(n: &UnitVector3) -> UnitVector3 {
    let a = n.into_inner().map(Real::abs);
    let helper = if a.x <= a.y && a.x <= a.z {
        Vector3::x()
    } else if a.y <= a.z {
        Vector3::y()
    } else {
        Vector3::z()
    };
    UnitVector3::new_normalize(n.cross(&helper))
}

/// An infinite plane with a local 2D coordinate system.
///
/// `P(u, v) = origin + u · x_axis + v · y_axis`, `y_axis = normal × x_axis`.
///
/// The `(u, v)` frame is what 2D sketches are drawn in.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Plane {
    /// The `(0, 0)` point of the plane.
    pub origin: Point3,
    /// Unit normal; defines the plane's positive side.
    pub normal: UnitVector3,
    /// In-plane `u` direction (⟂ `normal`).
    pub x_axis: UnitVector3,
}

impl Plane {
    /// Creates a plane; `x_hint` is projected into the plane (via
    /// Gram–Schmidt).
    pub fn new(origin: Point3, normal: Vector3, x_hint: Vector3) -> KernelResult<Self> {
        let (normal, x_axis) = orthonormal_frame(normal, x_hint)?;
        Ok(Self {
            origin,
            normal,
            x_axis,
        })
    }

    /// Creates a plane from origin and normal, picking any in-plane x-axis.
    pub fn from_normal(origin: Point3, normal: Vector3) -> KernelResult<Self> {
        Self::new(origin, normal, Vector3::zeros())
    }

    /// The world XY plane (normal +Z).
    pub fn xy() -> Self {
        Self {
            origin: Point3::origin(),
            normal: Vector3::z_axis(),
            x_axis: Vector3::x_axis(),
        }
    }

    /// The world YZ plane (normal +X).
    pub fn yz() -> Self {
        Self {
            origin: Point3::origin(),
            normal: Vector3::x_axis(),
            x_axis: Vector3::y_axis(),
        }
    }

    /// The world ZX plane (normal +Y).
    pub fn zx() -> Self {
        Self {
            origin: Point3::origin(),
            normal: Vector3::y_axis(),
            x_axis: Vector3::z_axis(),
        }
    }

    /// The in-plane `v` direction.
    pub fn y_axis(&self) -> UnitVector3 {
        UnitVector3::new_unchecked(self.normal.cross(&self.x_axis))
    }

    /// Maps plane coordinates `(u, v)` to a 3D point.
    pub fn point_at(&self, u: Real, v: Real) -> Point3 {
        self.origin + u * self.x_axis.into_inner() + v * self.y_axis().into_inner()
    }

    /// Projects a 3D point into plane coordinates `(u, v)`.
    pub fn project(&self, p: &Point3) -> na::Point2<Real> {
        let d = p - self.origin;
        na::Point2::new(d.dot(&self.x_axis), d.dot(&self.y_axis()))
    }

    /// Signed distance from `p` (positive on the `normal` side).
    pub fn signed_distance(&self, p: &Point3) -> Real {
        (p - self.origin).dot(&self.normal)
    }
}

impl Transformable for Plane {
    fn transform(&mut self, t: &Transform3) {
        self.origin = t.transform_point(&self.origin);
        self.normal = rotate_unit(t, &self.normal);
        self.x_axis = rotate_unit(t, &self.x_axis);
    }
}

/// An infinite circular cylinder (the side surface of extruded circles and
/// of holes).
///
/// `P(θ, h) = origin + r·(cos θ · x_axis + sin θ · y_axis) + h · axis`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cylinder {
    /// A point on the axis (the `h = 0` level).
    pub origin: Point3,
    /// Axis direction; also the direction of increasing `h`.
    pub axis: UnitVector3,
    /// Direction of `θ = 0` (⟂ `axis`).
    pub x_axis: UnitVector3,
    /// Radius (> 0).
    pub radius: Real,
}

impl Cylinder {
    /// Creates a cylinder; `x_hint` is projected perpendicular to `axis`.
    pub fn new(origin: Point3, axis: Vector3, x_hint: Vector3, radius: Real) -> KernelResult<Self> {
        if !(radius.is_finite() && radius > 0.0) {
            return Err(KernelError::Degenerate("cylinder radius must be positive"));
        }
        let (axis, x_axis) = orthonormal_frame(axis, x_hint)?;
        Ok(Self {
            origin,
            axis,
            x_axis,
            radius,
        })
    }

    /// Evaluates the surface at `(θ, h)`.
    pub fn point_at(&self, theta: Real, h: Real) -> Point3 {
        let y_axis = self.axis.cross(&self.x_axis);
        let (s, c) = theta.sin_cos();
        self.origin
            + self.radius * (c * self.x_axis.into_inner() + s * y_axis)
            + h * self.axis.into_inner()
    }

    /// Outward unit normal at angle `θ` (independent of `h`).
    pub fn normal_at(&self, theta: Real) -> UnitVector3 {
        let y_axis = self.axis.cross(&self.x_axis);
        let (s, c) = theta.sin_cos();
        UnitVector3::new_normalize(c * self.x_axis.into_inner() + s * y_axis)
    }
}

impl Transformable for Cylinder {
    fn transform(&mut self, t: &Transform3) {
        self.origin = t.transform_point(&self.origin);
        self.axis = rotate_unit(t, &self.axis);
        self.x_axis = rotate_unit(t, &self.x_axis);
    }
}

/// A circular cone (the side of a tapered extruded circle or arc):
///
/// `P(θ, h) = origin + (radius + h·tan(half_angle))·(cos θ · x_axis +
/// sin θ · y_axis) + h · axis`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cone {
    /// Centre of the reference circle (`h = 0`) on the axis.
    pub origin: Point3,
    /// Axis direction; also the direction of increasing `h`.
    pub axis: UnitVector3,
    /// Direction of `θ = 0` (⟂ `axis`).
    pub x_axis: UnitVector3,
    /// Radius at `h = 0` (> 0).
    pub radius: Real,
    /// Signed half-angle: positive widens along `axis`.
    pub half_angle: Real,
}

impl Cone {
    /// Creates a cone; `x_hint` is projected perpendicular to `axis`.
    pub fn new(
        origin: Point3,
        axis: Vector3,
        x_hint: Vector3,
        radius: Real,
        half_angle: Real,
    ) -> KernelResult<Self> {
        if !(radius.is_finite() && radius > 0.0) {
            return Err(KernelError::Degenerate("cone radius must be positive"));
        }
        if !(half_angle.is_finite() && half_angle.abs() < std::f64::consts::FRAC_PI_2) {
            return Err(KernelError::Degenerate(
                "cone half-angle must be within ±90°",
            ));
        }
        let (axis, x_axis) = orthonormal_frame(axis, x_hint)?;
        Ok(Self {
            origin,
            axis,
            x_axis,
            radius,
            half_angle,
        })
    }

    fn radial(&self, theta: Real) -> Vector3 {
        let y_axis = self.axis.cross(&self.x_axis);
        let (s, c) = theta.sin_cos();
        c * self.x_axis.into_inner() + s * y_axis
    }

    /// Evaluates the surface at `(θ, h)`.
    pub fn point_at(&self, theta: Real, h: Real) -> Point3 {
        self.origin
            + (self.radius + h * self.half_angle.tan()) * self.radial(theta)
            + h * self.axis.into_inner()
    }

    /// Unit normal pointing away from the axis.
    pub fn normal_at(&self, theta: Real) -> UnitVector3 {
        let (s, c) = self.half_angle.sin_cos();
        UnitVector3::new_normalize(c * self.radial(theta) - s * self.axis.into_inner())
    }
}

impl Transformable for Cone {
    fn transform(&mut self, t: &Transform3) {
        self.origin = t.transform_point(&self.origin);
        self.axis = rotate_unit(t, &self.axis);
        self.x_axis = rotate_unit(t, &self.x_axis);
    }
}

/// A curve swept along a straight direction (a "generalised cylinder"):
/// `P(u, v) = curve(u) + v · direction`. The side faces of extruded
/// splines.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExtrusionSurface {
    /// The swept curve (at `v = 0`).
    pub curve: BSplineCurve,
    /// Sweep direction (unit); `v` is the distance along it.
    pub direction: UnitVector3,
}

impl ExtrusionSurface {
    /// Evaluates the surface at `(u, v)`.
    pub fn point_at(&self, u: Real, v: Real) -> Point3 {
        self.curve.point_at(u) + v * self.direction.into_inner()
    }

    /// Unit normal `curve'(u) × direction` (independent of `v`).
    pub fn normal_at(&self, u: Real) -> UnitVector3 {
        UnitVector3::new_normalize(self.curve.derivative_at(u).cross(&self.direction))
    }
}

impl Transformable for ExtrusionSurface {
    fn transform(&mut self, t: &Transform3) {
        self.curve.transform(t);
        self.direction = rotate_unit(t, &self.direction);
    }
}

/// Any surface the kernel understands. B-spline (NURBS) surfaces, cones,
/// spheres and tori will be added as features require them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Surface {
    /// Infinite plane.
    Plane(Plane),
    /// Infinite circular cylinder.
    Cylinder(Cylinder),
    /// Circular cone.
    Cone(Cone),
    /// A curve swept along a direction.
    Extrusion(ExtrusionSurface),
    /// Sphere.
    Sphere(Sphere),
    /// Torus.
    Torus(Torus),
}

/// A sphere: `P(θ, φ) = center + r·(cos φ cos θ · x + cos φ sin θ · y + sin φ · z)`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sphere {
    /// Centre.
    pub center: Point3,
    /// Radius.
    pub radius: Real,
    /// Polar axis (`z`).
    pub axis: UnitVector3,
    /// `x` direction (⟂ axis).
    pub x_axis: UnitVector3,
}

/// A torus around `axis`: tube of `minor` radius around a circle of
/// `major` radius.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Torus {
    /// Centre.
    pub center: Point3,
    /// Axis.
    pub axis: UnitVector3,
    /// `x` direction (⟂ axis).
    pub x_axis: UnitVector3,
    /// Radius of the tube's centre circle.
    pub major: Real,
    /// Tube radius.
    pub minor: Real,
}

impl Surface {
    /// The outward unit normal of the surface (its own orientation) at the
    /// surface point nearest to `p` — the shading normal of facets of a
    /// curved surface. `None` where it is not defined simply (extrusion
    /// surfaces: use the facet normal).
    pub fn normal_near(&self, p: &Point3) -> Option<Vector3> {
        let radial = |origin: &Point3, axis: &UnitVector3| {
            let d = p - origin;
            (d - axis.into_inner() * d.dot(axis)).try_normalize(1e-12)
        };
        match self {
            Surface::Plane(s) => Some(s.normal.into_inner()),
            Surface::Cylinder(s) => radial(&s.origin, &s.axis),
            Surface::Cone(s) => {
                let r = radial(&s.origin, &s.axis)?;
                let (sn, cs) = s.half_angle.sin_cos();
                Some(cs * r - sn * s.axis.into_inner())
            }
            Surface::Sphere(s) => (p - s.center).try_normalize(1e-12),
            Surface::Torus(s) => {
                let r = radial(&s.center, &s.axis)?;
                let ring = s.center + r * s.major;
                (p - ring).try_normalize(1e-12)
            }
            Surface::Extrusion(_) => None,
        }
    }
    /// Evaluates the surface at `(u, v)`.
    pub fn point_at(&self, u: Real, v: Real) -> Point3 {
        match self {
            Surface::Plane(s) => s.point_at(u, v),
            Surface::Cylinder(s) => s.point_at(u, v),
            Surface::Cone(s) => s.point_at(u, v),
            Surface::Extrusion(s) => s.point_at(u, v),
            Surface::Sphere(s) => {
                let y = s.axis.cross(&s.x_axis);
                let (st, ct) = u.sin_cos();
                let (sp, cp) = v.sin_cos();
                s.center
                    + s.radius
                        * (cp * ct * s.x_axis.into_inner() + cp * st * y + sp * s.axis.into_inner())
            }
            Surface::Torus(s) => {
                let y = s.axis.cross(&s.x_axis);
                let (st, ct) = u.sin_cos();
                let (sp, cp) = v.sin_cos();
                let r = ct * s.x_axis.into_inner() + st * y;
                s.center + (s.major + s.minor * cp) * r + s.minor * sp * s.axis.into_inner()
            }
        }
    }

    /// Unit normal at `(u, v)` (the surface's own orientation; a face may
    /// flip it, see [`Face::sense`]).
    pub fn normal_at(&self, u: Real, _v: Real) -> UnitVector3 {
        match self {
            Surface::Plane(s) => s.normal,
            Surface::Cylinder(s) => s.normal_at(u),
            Surface::Cone(s) => s.normal_at(u),
            Surface::Extrusion(s) => s.normal_at(u),
            Surface::Sphere(_) | Surface::Torus(_) => {
                let p = self.point_at(u, _v);
                UnitVector3::new_normalize(self.normal_near(&p).unwrap_or(Vector3::z()))
            }
        }
    }
}

impl Transformable for Surface {
    fn transform(&mut self, t: &Transform3) {
        match self {
            Surface::Plane(s) => s.transform(t),
            Surface::Cylinder(s) => s.transform(t),
            Surface::Cone(s) => s.transform(t),
            Surface::Extrusion(s) => s.transform(t),
            Surface::Sphere(s) => {
                s.center = t.transform_point(&s.center);
                s.axis = rotate_unit(t, &s.axis);
                s.x_axis = rotate_unit(t, &s.x_axis);
            }
            Surface::Torus(s) => {
                s.center = t.transform_point(&s.center);
                s.axis = rotate_unit(t, &s.axis);
                s.x_axis = rotate_unit(t, &s.x_axis);
            }
        }
    }
}

// ============================================================================
// Topology
// ============================================================================

new_key_type! {
    /// Handle to a [`Curve`] stored in a [`BRep`].
    pub struct CurveId;
    /// Handle to a [`Surface`] stored in a [`BRep`].
    pub struct SurfaceId;
    /// Handle to a [`Vertex`].
    pub struct VertexId;
    /// Handle to an [`Edge`].
    pub struct EdgeId;
    /// Handle to a [`Face`].
    pub struct FaceId;
    /// Handle to a [`Solid`].
    pub struct SolidId;
}

/// Relative orientation of a topological entity to its underlying geometry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Sense {
    /// Same direction as the geometry.
    Same,
    /// Opposite direction to the geometry.
    Reversed,
}

impl Sense {
    /// The opposite sense.
    pub fn flipped(self) -> Self {
        match self {
            Sense::Same => Sense::Reversed,
            Sense::Reversed => Sense::Same,
        }
    }
}

/// A topological point: the end of one or more edges.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Vertex {
    /// The vertex position.
    pub point: Point3,
    /// Persistent name (see [`BRep::find_vertex`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// A bounded piece of a curve between two vertices.
///
/// Invariant (checked by [`BRep::add_edge`]): `curve(range.min) ≈ start` and
/// `curve(range.max) ≈ end`. A closed edge (full circle) has `start == end`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    /// Underlying curve.
    pub curve: CurveId,
    /// Vertex at `range.min`.
    pub start: VertexId,
    /// Vertex at `range.max`.
    pub end: VertexId,
    /// Portion of the curve used by this edge.
    pub range: Interval,
    /// Persistent name (see [`BRep::find_edge`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// The use of an [`Edge`] by a particular [`Loop`] ("half-edge").
///
/// In a closed manifold solid every edge is shared by exactly two faces and
/// is traversed once in each direction — once with [`Sense::Same`], once with
/// [`Sense::Reversed`]. Coedges make that explicit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Coedge {
    /// The edge being used.
    pub edge: EdgeId,
    /// Whether the loop traverses the edge start→end (`Same`) or end→start.
    pub sense: Sense,
}

/// A closed, ordered cycle of coedges bounding a face.
///
/// The end vertex of each coedge is the start vertex of the next, and the
/// last wraps around to the first.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Loop {
    /// Coedges in traversal order.
    pub coedges: Vec<Coedge>,
}

/// A bounded region of a surface.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Face {
    /// Underlying surface.
    pub surface: SurfaceId,
    /// Outer boundary, counter-clockwise seen from outside the solid.
    pub outer: Loop,
    /// Holes, clockwise seen from outside the solid.
    pub inner: Vec<Loop>,
    /// `Same` if the outward face normal equals the surface normal.
    pub sense: Sense,
    /// Persistent name (see [`BRep::find_face`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// For a planar facet approximating a curved face (after a Boolean or
    /// a revolve): the exact surface it approximates — used for smooth
    /// shading and to hide the edges between facets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub facet_of: Option<SurfaceId>,
    /// The face's triangles from the exact kernel (`kernel::exact`): used
    /// for display and picking instead of our own tessellation. Not saved
    /// (the body is rebuilt from the features).
    #[serde(skip)]
    pub mesh: Option<std::sync::Arc<TriangleMesh>>,
}

/// A connected set of faces. A closed shell encloses a volume.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Shell {
    /// Faces of the shell.
    pub faces: Vec<FaceId>,
}

/// A 3D volume, bounded by one outer shell and zero or more void shells
/// (internal cavities).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Solid {
    /// `shells[0]` is the outer boundary; the rest are cavities.
    pub shells: Vec<Shell>,
    /// An open surface body (Surface output), not a closed solid.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub open: bool,
    /// Persistent body name: the id of the feature that created it (rule
    /// 2; Inventor's "Solids" selection refers to bodies by it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Entity counts of a solid, used for the Euler–Poincaré check.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TopologyStats {
    /// Distinct vertices `V`.
    pub vertices: usize,
    /// Distinct edges `E`.
    pub edges: usize,
    /// Faces `F`.
    pub faces: usize,
    /// Inner loops (holes in faces) `R`.
    pub rings: usize,
    /// Shells `S`.
    pub shells: usize,
}

impl TopologyStats {
    /// Euler characteristic adjusted for face rings: `V − E + F − R`.
    pub fn euler_characteristic(&self) -> i64 {
        self.vertices as i64 - self.edges as i64 + self.faces as i64 - self.rings as i64
    }

    /// Total genus `H` (number of through-holes) from the Euler–Poincaré
    /// formula `V − E + F − R = 2(S − H)`, or `None` if the counts are
    /// inconsistent with any valid manifold solid.
    pub fn genus(&self) -> Option<usize> {
        let chi = self.euler_characteristic();
        if chi % 2 != 0 {
            return None;
        }
        let h = self.shells as i64 - chi / 2;
        usize::try_from(h).ok()
    }
}

// ============================================================================
// The B-Rep container
// ============================================================================

/// A complete boundary-representation model: geometry + topology.
///
/// This is the single source of truth for the 3D state of a document. It is
/// `Clone` (snapshotting for undo) and `Serialize` (files, agent API).
///
/// Mutation goes through `add_*` methods which validate references and
/// geometric consistency, so that a `BRep` cannot be put into a state where
/// an ID points nowhere.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BRep {
    /// Tolerances used by every check in this model.
    pub tolerance: Tolerance,
    curves: SlotMap<CurveId, Curve>,
    surfaces: SlotMap<SurfaceId, Surface>,
    vertices: SlotMap<VertexId, Vertex>,
    edges: SlotMap<EdgeId, Edge>,
    faces: SlotMap<FaceId, Face>,
    solids: SlotMap<SolidId, Solid>,
}

impl BRep {
    /// An empty model with default tolerances.
    pub fn new() -> Self {
        Self::default()
    }

    // ---- read access -------------------------------------------------------

    /// Looks up a curve.
    pub fn curve(&self, id: CurveId) -> Option<&Curve> {
        self.curves.get(id)
    }
    /// Looks up a surface.
    pub fn surface(&self, id: SurfaceId) -> Option<&Surface> {
        self.surfaces.get(id)
    }
    /// Looks up a vertex.
    pub fn vertex(&self, id: VertexId) -> Option<&Vertex> {
        self.vertices.get(id)
    }
    /// Looks up an edge.
    pub fn edge(&self, id: EdgeId) -> Option<&Edge> {
        self.edges.get(id)
    }
    /// Looks up a face.
    pub fn face(&self, id: FaceId) -> Option<&Face> {
        self.faces.get(id)
    }
    /// Sets a body's persistent name.
    pub fn name_solid(&mut self, id: SolidId, name: &str) {
        if let Some(s) = self.solids.get_mut(id) {
            s.name = Some(name.to_owned());
        }
    }
    /// Looks up a solid.
    pub fn solid(&self, id: SolidId) -> Option<&Solid> {
        self.solids.get(id)
    }
    /// Iterates over all solids.
    pub fn solids(&self) -> impl Iterator<Item = (SolidId, &Solid)> {
        self.solids.iter()
    }

    /// Axis-aligned box `(min, max)` around all vertices, or `None` for an
    /// empty model. Exact for polyhedra; curved faces may bulge beyond it.
    pub fn bounding_box(&self) -> Option<(Point3, Point3)> {
        let mut pts = self.vertices.values().map(|v| v.point);
        let first = pts.next()?;
        Some(pts.fold((first, first), |(lo, hi), p| (lo.inf(&p), hi.sup(&p))))
    }

    /// The vertex where `coedge` starts, taking its sense into account.
    pub fn coedge_start(&self, coedge: &Coedge) -> Option<VertexId> {
        let e = self.edges.get(coedge.edge)?;
        Some(match coedge.sense {
            Sense::Same => e.start,
            Sense::Reversed => e.end,
        })
    }

    /// The vertex where `coedge` ends, taking its sense into account.
    pub fn coedge_end(&self, coedge: &Coedge) -> Option<VertexId> {
        self.coedge_start(&Coedge {
            edge: coedge.edge,
            sense: coedge.sense.flipped(),
        })
    }

    // ---- construction ------------------------------------------------------

    /// Stores a curve.
    pub fn add_curve(&mut self, curve: Curve) -> CurveId {
        self.curves.insert(curve)
    }

    /// Marks `faces` as facets of the exact `surface` under one `name`
    /// (smooth shading, no edges drawn between them, one face for later
    /// Booleans).
    pub fn mark_facets(&mut self, faces: &[FaceId], surface: Surface, name: &str) {
        let s = self.add_surface(surface);
        for &f in faces {
            if let Some(face) = self.faces.get_mut(f) {
                face.facet_of = Some(s);
                face.name = Some(name.to_owned());
            }
        }
    }

    /// Stores a surface.
    pub fn add_surface(&mut self, surface: Surface) -> SurfaceId {
        self.surfaces.insert(surface)
    }

    /// Creates a vertex.
    pub fn add_vertex(&mut self, point: Point3) -> VertexId {
        self.vertices.insert(Vertex { point, name: None })
    }

    /// Creates an edge, checking that the curve passes through both vertices
    /// at the ends of `range`.
    pub fn add_edge(
        &mut self,
        curve: CurveId,
        start: VertexId,
        end: VertexId,
        range: Interval,
    ) -> KernelResult<EdgeId> {
        let c = self
            .curves
            .get(curve)
            .ok_or(KernelError::DanglingReference("curve"))?;
        let ps = self
            .vertices
            .get(start)
            .ok_or(KernelError::DanglingReference("vertex"))?
            .point;
        let pe = self
            .vertices
            .get(end)
            .ok_or(KernelError::DanglingReference("vertex"))?
            .point;

        if let Some(domain) = c.domain()
            && !(domain.contains(range.min, 1e-12) && domain.contains(range.max, 1e-12))
        {
            return Err(KernelError::InvalidTopology(
                "edge range exceeds curve domain".into(),
            ));
        }
        if !self.tolerance.points_coincide(&c.point_at(range.min), &ps) {
            return Err(KernelError::InvalidTopology(
                "edge start vertex is not on its curve".into(),
            ));
        }
        if !self.tolerance.points_coincide(&c.point_at(range.max), &pe) {
            return Err(KernelError::InvalidTopology(
                "edge end vertex is not on its curve".into(),
            ));
        }
        Ok(self.edges.insert(Edge {
            curve,
            start,
            end,
            range,
            name: None,
        }))
    }

    /// Convenience: creates a straight edge (and its [`Line`]) between two
    /// existing vertices. The line is parameterised by arc length.
    pub fn add_line_edge(&mut self, start: VertexId, end: VertexId) -> KernelResult<EdgeId> {
        let a = self
            .vertices
            .get(start)
            .ok_or(KernelError::DanglingReference("vertex"))?
            .point;
        let b = self
            .vertices
            .get(end)
            .ok_or(KernelError::DanglingReference("vertex"))?
            .point;
        let line = Line::through_points(a, b, &self.tolerance)?;
        let range = Interval::new(0.0, na::distance(&a, &b))?;
        let curve = self.add_curve(Curve::Line(line));
        self.add_edge(curve, start, end, range)
    }

    /// Checks that a loop is non-empty, references live edges, and is closed
    /// (each coedge ends where the next begins).
    pub fn check_loop(&self, lp: &Loop) -> KernelResult<()> {
        if lp.coedges.is_empty() {
            return Err(KernelError::InvalidTopology("empty loop".into()));
        }
        for (i, ce) in lp.coedges.iter().enumerate() {
            let next = &lp.coedges[(i + 1) % lp.coedges.len()];
            let end = self
                .coedge_end(ce)
                .ok_or(KernelError::DanglingReference("edge"))?;
            let start = self
                .coedge_start(next)
                .ok_or(KernelError::DanglingReference("edge"))?;
            if end != start {
                return Err(KernelError::InvalidTopology(format!(
                    "loop is open between coedge {i} and {}",
                    (i + 1) % lp.coedges.len()
                )));
            }
        }
        Ok(())
    }

    /// Creates a face after validating its surface and all of its loops.
    pub fn add_face(
        &mut self,
        surface: SurfaceId,
        outer: Loop,
        inner: Vec<Loop>,
        sense: Sense,
    ) -> KernelResult<FaceId> {
        if !self.surfaces.contains_key(surface) {
            return Err(KernelError::DanglingReference("surface"));
        }
        self.check_loop(&outer)?;
        for lp in &inner {
            self.check_loop(lp)?;
        }
        Ok(self.faces.insert(Face {
            surface,
            outer,
            inner,
            sense,
            name: None,
            facet_of: None,
            mesh: None,
        }))
    }

    /// Creates a solid from shells, after checking each shell is closed and
    /// manifold (see [`BRep::check_closed_shell`]).
    pub fn add_solid(&mut self, shells: Vec<Shell>) -> KernelResult<SolidId> {
        if shells.is_empty() {
            return Err(KernelError::InvalidTopology("solid has no shells".into()));
        }
        for shell in &shells {
            self.check_closed_shell(shell)?;
        }
        Ok(self.solids.insert(Solid {
            shells,
            open: false,
            name: None,
        }))
    }

    // ---- validation --------------------------------------------------------

    /// Checks that a shell is a closed 2-manifold: every edge is used by
    /// exactly two coedges, once in each direction.
    ///
    /// This is the property that makes "inside" and "outside" well defined,
    /// i.e. that makes a shell watertight.
    pub fn check_closed_shell(&self, shell: &Shell) -> KernelResult<()> {
        if shell.faces.is_empty() {
            return Err(KernelError::InvalidTopology("shell has no faces".into()));
        }
        // edge -> (uses with Same, uses with Reversed)
        let mut uses: HashMap<EdgeId, (u32, u32)> = HashMap::new();
        for &fid in &shell.faces {
            let face = self
                .faces
                .get(fid)
                .ok_or(KernelError::DanglingReference("face"))?;
            for lp in std::iter::once(&face.outer).chain(&face.inner) {
                for ce in &lp.coedges {
                    let entry = uses.entry(ce.edge).or_default();
                    match ce.sense {
                        Sense::Same => entry.0 += 1,
                        Sense::Reversed => entry.1 += 1,
                    }
                }
            }
        }
        for (edge, (same, rev)) in uses {
            if (same, rev) != (1, 1) {
                return Err(KernelError::InvalidTopology(format!(
                    "edge {edge:?} is not manifold (used {same}× forward, {rev}× reversed)"
                )));
            }
        }
        Ok(())
    }

    /// Counts the topological entities of a solid.
    pub fn stats(&self, solid: SolidId) -> KernelResult<TopologyStats> {
        let s = self
            .solids
            .get(solid)
            .ok_or(KernelError::DanglingReference("solid"))?;
        let mut vertices = std::collections::HashSet::new();
        let mut edges = std::collections::HashSet::new();
        let mut stats = TopologyStats {
            shells: s.shells.len(),
            ..Default::default()
        };
        for shell in &s.shells {
            for &fid in &shell.faces {
                let face = self
                    .faces
                    .get(fid)
                    .ok_or(KernelError::DanglingReference("face"))?;
                stats.faces += 1;
                stats.rings += face.inner.len();
                for lp in std::iter::once(&face.outer).chain(&face.inner) {
                    for ce in &lp.coedges {
                        let e = self
                            .edges
                            .get(ce.edge)
                            .ok_or(KernelError::DanglingReference("edge"))?;
                        edges.insert(ce.edge);
                        vertices.insert(e.start);
                        vertices.insert(e.end);
                    }
                }
            }
        }
        stats.vertices = vertices.len();
        stats.edges = edges.len();
        Ok(stats)
    }

    /// Full consistency check of a solid: closed manifold shells and a valid
    /// Euler–Poincaré characteristic.
    pub fn validate_solid(&self, solid: SolidId) -> KernelResult<TopologyStats> {
        let s = self
            .solids
            .get(solid)
            .ok_or(KernelError::DanglingReference("solid"))?;
        for shell in &s.shells {
            self.check_closed_shell(shell)?;
        }
        let stats = self.stats(solid)?;
        if stats.genus().is_none() {
            return Err(KernelError::InvalidTopology(format!(
                "Euler–Poincaré check failed: V−E+F−R = {}",
                stats.euler_characteristic()
            )));
        }
        Ok(stats)
    }

    // ---- whole-model operations -------------------------------------------

    /// Applies a rigid transform to every point, curve and surface in the
    /// model. Topology is unaffected by rigid motions.
    pub fn transform(&mut self, t: &Transform3) {
        self.vertices
            .values_mut()
            .for_each(|v| v.point.transform(t));
        self.curves.values_mut().for_each(|c| c.transform(t));
        self.surfaces.values_mut().for_each(|s| s.transform(t));
    }

    // ---- primitives --------------------------------------------------------

    /// Builds an axis-aligned box spanning the corners `a` and `b` (in any
    /// order) as a fully connected B-Rep solid: 8 vertices, 12 edges,
    /// 6 planar faces, 1 shell.
    ///
    /// This is both a useful primitive and the reference example of how to
    /// assemble valid topology with the `add_*` API.
    pub fn make_box(&mut self, a: Point3, b: Point3) -> KernelResult<SolidId> {
        self.make_box_named(a, b, "Box")
    }

    /// [`BRep::make_box`] with persistent names prefixed by `prefix`
    /// (the feature name, e.g. `"Box1"`):
    ///
    /// - faces `"<prefix>:Bottom" | Top | Front (−Y) | Back | Left (−X) | Right`;
    /// - vertices `"<prefix>:V<x><y><z>"`, each digit 0 = min, 1 = max side;
    /// - edges `"<prefix>:E<v1>-<v2>"` from their two vertex codes.
    ///
    /// The names do not depend on the box size, so a sketch placed on
    /// `Box1:Top` finds that face again after the box is resized.
    pub fn make_box_named(&mut self, a: Point3, b: Point3, prefix: &str) -> KernelResult<SolidId> {
        let min = a.inf(&b);
        let max = a.sup(&b);
        let size = max - min;
        if size.iter().any(|&s| s <= self.tolerance.linear) {
            return Err(KernelError::Degenerate("box has zero extent along an axis"));
        }

        // Corner i has coordinate bits (x, y, z) = (i & 1, i >> 1 & 1, i >> 2 & 1).
        let corner = |i: usize| {
            Point3::new(
                if i & 1 == 0 { min.x } else { max.x },
                if i & 2 == 0 { min.y } else { max.y },
                if i & 4 == 0 { min.z } else { max.z },
            )
        };
        let code = |i: usize| format!("{}{}{}", i & 1, (i >> 1) & 1, (i >> 2) & 1);
        let v: Vec<VertexId> = (0..8)
            .map(|i| {
                let id = self.add_vertex(corner(i));
                self.vertices[id].name = Some(format!("{prefix}:V{}", code(i)));
                id
            })
            .collect();

        // Each face as a CCW corner cycle seen from outside, with its outward normal.
        const FACES: [(&str, [usize; 4], [Real; 3]); 6] = [
            ("Bottom", [0, 2, 3, 1], [0.0, 0.0, -1.0]), // z = min
            ("Top", [4, 5, 7, 6], [0.0, 0.0, 1.0]),     // z = max
            ("Front", [0, 1, 5, 4], [0.0, -1.0, 0.0]),  // y = min
            ("Back", [2, 6, 7, 3], [0.0, 1.0, 0.0]),    // y = max
            ("Left", [0, 4, 6, 2], [-1.0, 0.0, 0.0]),   // x = min
            ("Right", [1, 3, 7, 5], [1.0, 0.0, 0.0]),   // x = max
        ];

        // Each geometric edge is shared by two faces; create it once, keyed by
        // its unordered corner pair, and record the direction it was created in.
        let mut edge_of: HashMap<(usize, usize), EdgeId> = HashMap::new();
        let mut faces = Vec::with_capacity(6);

        for (face_name, cycle, n) in FACES {
            let mut coedges = Vec::with_capacity(4);
            for k in 0..4 {
                let (from, to) = (cycle[k], cycle[(k + 1) % 4]);
                let key = (from.min(to), from.max(to));
                let edge = match edge_of.get(&key) {
                    Some(&e) => e,
                    None => {
                        let e = self.add_line_edge(v[key.0], v[key.1])?;
                        self.edges[e].name =
                            Some(format!("{prefix}:E{}-{}", code(key.0), code(key.1)));
                        edge_of.insert(key, e);
                        e
                    }
                };
                let sense = if from == key.0 {
                    Sense::Same
                } else {
                    Sense::Reversed
                };
                coedges.push(Coedge { edge, sense });
            }

            let p0 = corner(cycle[0]);
            let x_hint = corner(cycle[1]) - p0;
            let plane = Plane::new(p0, Vector3::from(n), x_hint)?;
            let surface = self.add_surface(Surface::Plane(plane));
            let face = self.add_face(surface, Loop { coedges }, Vec::new(), Sense::Same)?;
            self.faces[face].name = Some(format!("{prefix}:{face_name}"));
            faces.push(face);
        }

        self.add_solid(vec![Shell { faces }])
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_2};

    #[test]
    fn line_projection_and_distance() {
        let tol = Tolerance::default();
        let l = Line::through_points(Point3::origin(), Point3::new(10.0, 0.0, 0.0), &tol).unwrap();
        let p = Point3::new(3.0, 4.0, 0.0);
        assert_relative_eq!(l.parameter_of(&p), 3.0);
        assert_relative_eq!(l.distance_to(&p), 4.0);
        assert!(Line::through_points(p, p, &tol).is_err());
    }

    #[test]
    fn circle_evaluation_and_tangent() {
        let c = Circle::new(Point3::origin(), Vector3::z(), Vector3::x(), 2.0).unwrap();
        assert_relative_eq!(
            c.point_at(FRAC_PI_2),
            Point3::new(0.0, 2.0, 0.0),
            epsilon = 1e-12
        );
        let t = Curve::Circle(c).tangent_at(0.0).unwrap();
        assert_relative_eq!(t.into_inner(), Vector3::y(), epsilon = 1e-12);
    }

    #[test]
    fn bspline_validation() {
        let pts = vec![Point3::origin(), Point3::new(1.0, 0.0, 0.0)];
        assert!(BSplineCurve::new(1, pts.clone(), vec![0.0, 0.0, 1.0, 1.0], None).is_ok());
        assert!(BSplineCurve::new(1, pts.clone(), vec![0.0, 1.0, 1.0], None).is_err()); // wrong count
        assert!(BSplineCurve::new(1, pts.clone(), vec![0.0, 1.0, 0.5, 1.0], None).is_err()); // decreasing
        assert!(
            BSplineCurve::new(
                1,
                pts.clone(),
                vec![0.0, 0.0, 1.0, 1.0],
                Some(vec![1.0, 0.0])
            )
            .is_err()
        );
        assert!(BSplineCurve::new(2, pts, vec![0.0; 5], None).is_err()); // too few points
    }

    #[test]
    fn quadratic_bezier_matches_closed_form() {
        // A clamped degree-2 spline with 3 points is a quadratic Bézier:
        // B(t) = (1−t)²P0 + 2t(1−t)P1 + t²P2.
        let p = [
            Point3::origin(),
            Point3::new(1.0, 2.0, 0.0),
            Point3::new(2.0, 0.0, 1.0),
        ];
        let c = BSplineCurve::clamped_uniform(2, p.to_vec()).unwrap();
        for &t in &[0.0_f64, 0.25, 0.5, 0.8, 1.0] {
            let expected = p[0].coords * (1.0 - t).powi(2)
                + p[1].coords * 2.0 * t * (1.0 - t)
                + p[2].coords * t * t;
            assert_relative_eq!(c.point_at(t).coords, expected, epsilon = 1e-12);
            let d_expected = (p[1] - p[0]) * 2.0 * (1.0 - t) + (p[2] - p[1]) * 2.0 * t;
            assert_relative_eq!(c.derivative_at(t), d_expected, epsilon = 1e-12);
        }
    }

    #[test]
    fn rational_bspline_represents_circle_exactly() {
        // Quarter unit circle as a rational quadratic with middle weight √2/2.
        let c = BSplineCurve::new(
            2,
            vec![
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(1.0, 1.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
            ],
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            Some(vec![1.0, FRAC_1_SQRT_2, 1.0]),
        )
        .unwrap();
        for i in 0..=20 {
            let t = i as Real / 20.0;
            let p = c.point_at(t);
            assert_relative_eq!(p.coords.norm(), 1.0, epsilon = 1e-12);
            // On a circle the tangent is perpendicular to the radius.
            assert_relative_eq!(c.derivative_at(t).dot(&p.coords), 0.0, epsilon = 1e-12);
        }
    }

    #[test]
    fn bspline_multi_span_is_continuous_at_knots() {
        let pts = (0..6)
            .map(|i| Point3::new(i as Real, ((i * 7) % 3) as Real, 0.0))
            .collect();
        let c = BSplineCurve::clamped_uniform(3, pts).unwrap();
        let knot = c.knots()[4];
        assert_relative_eq!(
            c.point_at(knot - 1e-9),
            c.point_at(knot + 1e-9),
            epsilon = 1e-7
        );
        assert_relative_eq!(c.point_at(1.0), Point3::new(5.0, 2.0, 0.0), epsilon = 1e-12);
    }

    #[test]
    fn box_is_a_valid_closed_solid() {
        let mut m = BRep::new();
        let id = m
            .make_box(Point3::new(10.0, 20.0, 30.0), Point3::origin())
            .unwrap();
        let s = m.validate_solid(id).unwrap();
        assert_eq!((s.vertices, s.edges, s.faces, s.shells), (8, 12, 6, 1));
        assert_eq!(s.genus(), Some(0));

        // Every face normal must point away from the box centre.
        let centre = Point3::new(5.0, 10.0, 15.0);
        for &fid in &m.solid(id).unwrap().shells[0].faces {
            let Surface::Plane(pl) = m.surface(m.face(fid).unwrap().surface).unwrap() else {
                panic!("box faces are planar");
            };
            assert!(pl.signed_distance(&centre) < 0.0);
        }
    }

    #[test]
    fn open_shell_is_rejected() {
        let mut m = BRep::new();
        let id = m
            .make_box(Point3::origin(), Point3::new(1.0, 1.0, 1.0))
            .unwrap();
        let mut shell = m.solid(id).unwrap().shells[0].clone();
        shell.faces.pop();
        assert!(m.add_solid(vec![shell]).is_err());
    }

    #[test]
    fn edge_must_lie_on_curve() {
        let mut m = BRep::new();
        let a = m.add_vertex(Point3::origin());
        let b = m.add_vertex(Point3::new(1.0, 0.0, 0.0));
        let off = m.add_vertex(Point3::new(1.0, 1.0, 0.0));
        let line = m.add_curve(Curve::Line(
            Line::new(Point3::origin(), Vector3::x()).unwrap(),
        ));
        let r = Interval::new(0.0, 1.0).unwrap();
        assert!(m.add_edge(line, a, b, r).is_ok());
        assert!(m.add_edge(line, a, off, r).is_err());
    }

    #[test]
    fn rigid_transform_moves_model() {
        let mut m = BRep::new();
        let id = m
            .make_box(Point3::origin(), Point3::new(1.0, 1.0, 1.0))
            .unwrap();
        let t = Transform3::new(Vector3::new(5.0, 0.0, 0.0), Vector3::z() * FRAC_PI_2);
        m.transform(&t);
        m.validate_solid(id).unwrap();
        // Corner (1, 0, 0) → rotated 90° about Z → (0, 1, 0) → translated → (5, 1, 0).
        assert!(m.vertices.values().any(|v| {
            m.tolerance
                .points_coincide(&v.point, &Point3::new(5.0, 1.0, 0.0))
        }));
    }

    #[test]
    fn model_roundtrips_through_json() {
        let mut m = BRep::new();
        let id = m
            .make_box(Point3::origin(), Point3::new(1.0, 2.0, 3.0))
            .unwrap();
        let json = serde_json::to_string(&m).unwrap();
        let back: BRep = serde_json::from_str(&json).unwrap();
        assert_eq!(
            back.validate_solid(id).unwrap(),
            m.validate_solid(id).unwrap()
        );
    }
}
