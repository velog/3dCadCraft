//! Interpolation splines (Inventor's **Spline Interpolation**,
//! `docs/spec/1.2a-splines.md`): a cubic C2 B-spline that passes *through*
//! its points.
//!
//! An interpolation spline is an [`Entity::Spline`] whose point list holds
//! the fit points, marked in [`Sketch::interp`] with its end handles. Its
//! control points are computed from the fit points by global interpolation
//! (uniform parameters, clamped uniform knots — the representation every
//! other part of the program already uses for splines). They are an affine
//! function of the fit points and handles, so the solver, profiles,
//! extrusions and patterns work unchanged.
//!
//! End conditions: a **handle** (tangent `H`, in mm per segment: the
//! curve's derivative with respect to a parameter that grows by 1 per span)
//! or, without one, a **natural** end (zero curvature).

use serde::{Deserialize, Serialize};

use super::{Entity, EntityId, PointId, Sketch, SketchError, Vector2};
use crate::kernel::Real;

/// Fit data of an interpolation spline.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SplineFit {
    /// Tangent handle at the first point (`None` = natural end).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<[Real; 2]>,
    /// Tangent handle at the last point.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<[Real; 2]>,
    /// Handles at the interior fit points, by index in the point list
    /// (shorter or empty = none). Indices 0 and the last are unused.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inner: Vec<Option<[Real; 2]>>,
    /// Closed curve (Inventor's Close Curve): the point list repeats its
    /// first point at the end and the curve is C2 across it (C1 when the
    /// closing point has a handle).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub closed: bool,
}

impl SplineFit {
    /// The handle at point index `k` of `n`.
    pub fn handle(&self, k: usize, n: usize) -> Option<Vector2> {
        let v = if k == 0 || (self.closed && k + 1 == n) {
            self.start
        } else if k + 1 == n {
            self.end
        } else {
            self.inner.get(k).copied().flatten()
        };
        v.map(|v| Vector2::new(v[0], v[1]))
    }

    fn set_handle(&mut self, k: usize, n: usize, h: Option<Vector2>) {
        let v = h.map(|h| [h.x, h.y]);
        if k == 0 || (self.closed && k + 1 == n) {
            self.start = v;
        } else if k + 1 == n {
            self.end = v;
        } else {
            if self.inner.len() <= k {
                self.inner.resize(k + 1, None);
            }
            self.inner[k] = v;
            while self.inner.last() == Some(&None) {
                self.inner.pop();
            }
        }
    }

    /// Whether any handle is set.
    pub fn has_handles(&self) -> bool {
        self.start.is_some() || self.end.is_some() || self.inner.iter().any(Option::is_some)
    }
}

/// Chord-length parameters of the fit points, in [0, 1].
fn chord_params(fit: &[Vector2]) -> Vec<Real> {
    let n = fit.len();
    let mut u = vec![0.0; n];
    for k in 1..n {
        u[k] = u[k - 1] + (fit[k] - fit[k - 1]).norm();
    }
    let total = u[n - 1];
    if total <= 1e-12 {
        return (0..n).map(|k| k as Real / (n - 1).max(1) as Real).collect();
    }
    u.iter().map(|x| x / total).collect()
}

/// `k`-th derivative of the B-spline basis function `N_{i,p}(u)`
/// (Cox–de Boor). At the last knot the last non-empty span counts, so the
/// curve's end is evaluated from the left.
fn dbasis(t: &[Real], p: usize, i: usize, u: Real, k: usize) -> Real {
    if k == 0 && p == 0 {
        let last = t[t.len() - 1];
        let inside = t[i] <= u && u < t[i + 1];
        let at_end = u >= last && t[i] < t[i + 1] && t[i + 1] >= last;
        return if inside || at_end { 1.0 } else { 0.0 };
    }
    if p == 0 {
        return 0.0;
    }
    let (wa, wb) = if k == 0 {
        let d1 = t[i + p] - t[i];
        let d2 = t[i + p + 1] - t[i + 1];
        (
            if d1 > 0.0 { (u - t[i]) / d1 } else { 0.0 },
            if d2 > 0.0 {
                (t[i + p + 1] - u) / d2
            } else {
                0.0
            },
        )
    } else {
        let d1 = t[i + p] - t[i];
        let d2 = t[i + p + 1] - t[i + 1];
        (
            if d1 > 0.0 { p as Real / d1 } else { 0.0 },
            if d2 > 0.0 { -(p as Real) / d2 } else { 0.0 },
        )
    };
    let kk = k.saturating_sub(1);
    let a = if wa != 0.0 {
        wa * dbasis(t, p - 1, i, u, kk)
    } else {
        0.0
    };
    let b = if wb != 0.0 {
        wb * dbasis(t, p - 1, i + 1, u, kk)
    } else {
        0.0
    };
    a + b
}

/// Row of `k`-th derivative coefficients of all `m` control points at `u`.
fn row(t: &[Real], m: usize, u: Real, k: usize) -> Vec<(usize, Real)> {
    (0..m)
        .map(|j| (j, dbasis(t, 3, j, u, k)))
        .filter(|(_, c)| *c != 0.0)
        .collect()
}

/// `k`-th derivative of the cubic B-spline (`cv`, full knots `t`) at `u`.
pub(crate) fn derivative(cv: &[Vector2], t: &[Real], u: Real, k: usize) -> Vector2 {
    row(t, cv.len(), u, k)
        .into_iter()
        .map(|(j, c)| cv[j] * c)
        .sum()
}

/// The cubic spline through `fit` (≥ 2 points): control points and full
/// knot vector. Chord-length parameters, knots at the fit points (the
/// classic cubic spline); a point with a handle gets a double knot (C1
/// there, the tangent is the handle). A handle `H` is dimensionless: the
/// derivative is `H · L` with `L` the total chord length (for an end:
/// `Q1 = P0 + H · chord0 / 3`). Free ends are natural (zero curvature);
/// a closed curve is periodic (equal first and second derivatives).
pub fn interpolate_fit(fit: &[Vector2], data: &SplineFit) -> (Vec<Vector2>, Vec<Real>) {
    let n = fit.len();
    if n < 2 {
        return (fit.to_vec(), Vec::new());
    }
    let h = |k: usize| data.handle(k, n);
    if n == 2 {
        let c0 = (fit[1] - fit[0]).norm();
        let dir = (fit[1] - fit[0]) / c0.max(1e-12);
        let (a, b) = (h(0).unwrap_or(dir), h(1).unwrap_or(dir));
        return (
            vec![fit[0], fit[0] + a * c0 / 3.0, fit[1] - b * c0 / 3.0, fit[1]],
            vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        );
    }
    let u = chord_params(fit);
    let total: Real = fit.windows(2).map(|w| (w[1] - w[0]).norm()).sum();
    let mut t = vec![0.0; 4];
    for (k, &uk) in u.iter().enumerate().take(n - 1).skip(1) {
        t.push(uk);
        if h(k).is_some() {
            t.push(uk);
        }
    }
    t.extend(std::iter::repeat_n(1.0, 4));
    let m = t.len() - 4;
    let mut a = nalgebra::DMatrix::<Real>::zeros(m, m);
    let mut bx = nalgebra::DVector::<Real>::zeros(m);
    let mut by = nalgebra::DVector::<Real>::zeros(m);
    let mut r = 0;
    let mut put = |coefs: &[(usize, Real)], rhs: Vector2| {
        for &(j, c) in coefs {
            a[(r, j)] += c;
        }
        bx[r] = rhs.x;
        by[r] = rhs.y;
        r += 1;
    };
    for (k, q) in fit.iter().enumerate() {
        // Clamped ends: the curve starts/ends at the first/last control point.
        let coefs = match k {
            0 => vec![(0, 1.0)],
            _ if k == n - 1 => vec![(m - 1, 1.0)],
            _ => row(&t, m, u[k], 0),
        };
        put(&coefs, *q);
    }
    for (k, &uk) in u.iter().enumerate().take(n - 1).skip(1) {
        if let Some(hk) = h(k) {
            put(&row(&t, m, uk, 1), hk * total);
        }
    }
    let diff = |a: Vec<(usize, Real)>, b: Vec<(usize, Real)>| {
        let mut v = a;
        v.extend(b.into_iter().map(|(j, c)| (j, -c)));
        v
    };
    if data.closed {
        match h(0) {
            Some(h0) => {
                put(&row(&t, m, 0.0, 1), h0 * total);
                put(&row(&t, m, 1.0, 1), h0 * total);
            }
            None => {
                put(
                    &diff(row(&t, m, 0.0, 1), row(&t, m, 1.0, 1)),
                    Vector2::zeros(),
                );
                put(
                    &diff(row(&t, m, 0.0, 2), row(&t, m, 1.0, 2)),
                    Vector2::zeros(),
                );
            }
        }
    } else {
        for (k, uk) in [(0, 0.0), (n - 1, 1.0)] {
            match h(k) {
                Some(hk) => put(&row(&t, m, uk, 1), hk * total),
                None => put(&row(&t, m, uk, 2), Vector2::zeros()),
            }
        }
    }
    let lu = a.lu();
    match (lu.solve(&bx), lu.solve(&by)) {
        (Some(x), Some(y)) => ((0..m).map(|j| Vector2::new(x[j], y[j])).collect(), t),
        _ => (fit.to_vec(), Vec::new()),
    }
}

/// [`interpolate_fit`] with only end handles (open curve).
#[cfg(test)]
fn interpolate(
    fit: &[Vector2],
    start: Option<Vector2>,
    end: Option<Vector2>,
) -> (Vec<Vector2>, Vec<Real>) {
    let data = SplineFit {
        start: start.map(|v| [v.x, v.y]),
        end: end.map(|v| [v.x, v.y]),
        ..Default::default()
    };
    interpolate_fit(fit, &data)
}

impl Sketch {
    /// Adds an **interpolation spline** through existing points (≥ 2).
    pub fn add_interp_spline(&mut self, points: Vec<PointId>) -> Result<EntityId, SketchError> {
        let id = self.add_spline(points)?;
        self.interp.insert(id, SplineFit::default());
        Ok(id)
    }

    /// Replaces the fit data of an interpolation spline (projected splines
    /// copy their source's).
    pub fn set_spline_fit(&mut self, id: EntityId, fit: SplineFit) -> Result<(), SketchError> {
        self.spline_points(id)?;
        self.interp.insert(id, fit);
        Ok(())
    }

    /// Whether a spline passes through its points (else control vertex).
    pub fn is_interp_spline(&self, id: EntityId) -> bool {
        self.interp.contains_key(id)
    }

    /// Fit data of an interpolation spline.
    pub fn spline_fit(&self, id: EntityId) -> Option<&SplineFit> {
        self.interp.get(id)
    }

    fn spline_points(&self, id: EntityId) -> Result<&Vec<PointId>, SketchError> {
        match self.entities.get(id) {
            Some(Entity::Spline { points }) => Ok(points),
            _ => Err(SketchError::UnknownEntity),
        }
    }

    /// Sets (or with `None` resets to natural) the tangent handle at the
    /// start (`end == false`) or end of an interpolation spline.
    pub fn set_spline_handle(
        &mut self,
        id: EntityId,
        end: bool,
        handle: Option<Vector2>,
    ) -> Result<(), SketchError> {
        let n = self.spline_points(id)?.len();
        self.set_spline_handle_at(id, if end { n - 1 } else { 0 }, handle)
    }

    /// Sets (`None`: removes) the handle at point index `k`.
    pub fn set_spline_handle_at(
        &mut self,
        id: EntityId,
        k: usize,
        handle: Option<Vector2>,
    ) -> Result<(), SketchError> {
        let n = self.spline_points(id)?.len();
        if k >= n {
            return Err(SketchError::InvalidValue("no such spline point"));
        }
        let f = self
            .interp
            .get_mut(id)
            .ok_or(SketchError::InvalidValue("not an interpolation spline"))?;
        f.set_handle(k, n, handle);
        Ok(())
    }

    /// Removes every handle (Inventor's Reset All Handles).
    pub fn reset_spline_handles(&mut self, id: EntityId) -> Result<(), SketchError> {
        let f = self
            .interp
            .get_mut(id)
            .ok_or(SketchError::InvalidValue("not an interpolation spline"))?;
        f.start = None;
        f.end = None;
        f.inner.clear();
        Ok(())
    }

    /// **Activate Handle** at point index `k`: a handle with the curve's
    /// present tangent, so the shape does not jump.
    pub fn activate_spline_handle(&mut self, id: EntityId, k: usize) -> Result<(), SketchError> {
        let points = self.spline_points(id)?.clone();
        let fit: Vec<Vector2> = points
            .iter()
            .map(|&p| self.points.get(p).map(|q| q.position.coords))
            .collect::<Option<_>>()
            .ok_or(SketchError::UnknownPoint)?;
        let (cv, t) = self.spline_def(id, &|p| self.points[p].position.coords);
        if t.is_empty() || k >= fit.len() {
            return Err(SketchError::InvalidValue("not an interpolation spline"));
        }
        let u = chord_params(&fit)[k];
        let total: Real = fit.windows(2).map(|w| (w[1] - w[0]).norm()).sum();
        let d = derivative(&cv, &t, u, 1) / total.max(1e-12);
        self.set_spline_handle_at(id, k, Some(d))
    }

    /// Length scale of the grip at point `k`: grip = point + H · scale.
    fn grip_scale(fit: &[Vector2], k: usize, closed: bool) -> Real {
        let n = fit.len();
        let c = |a: usize| (fit[a + 1] - fit[a]).norm();
        if closed && (k == 0 || k + 1 == n) {
            (c(0) + c(n - 2)) / 6.0
        } else if k == 0 {
            c(0) / 3.0
        } else if k + 1 == n {
            c(n - 2) / 3.0
        } else {
            (c(k - 1) + c(k)) / 6.0
        }
    }

    /// The handle at point index `k`: (point, grip). Without a handle the
    /// grip shows the curve's present tangent there.
    pub fn spline_handle_grip_at(
        &self,
        id: EntityId,
        k: usize,
    ) -> Option<(super::Point2, super::Point2)> {
        let points = self.spline_points(id).ok()?;
        let data = self.interp.get(id)?;
        let fit: Vec<Vector2> = points
            .iter()
            .map(|&p| self.points.get(p).map(|q| q.position.coords))
            .collect::<Option<_>>()?;
        let n = fit.len();
        if k >= n || n < 2 {
            return None;
        }
        let total: Real = fit.windows(2).map(|w| (w[1] - w[0]).norm()).sum();
        let h = match data.handle(k, n) {
            Some(h) => h,
            None => {
                let (cv, t) = self.spline_def(id, &|p| self.points[p].position.coords);
                if t.is_empty() {
                    return None;
                }
                derivative(&cv, &t, chord_params(&fit)[k], 1) / total.max(1e-12)
            }
        };
        let base = super::Point2::from(fit[k]);
        Some((base, base + h * Self::grip_scale(&fit, k, data.closed)))
    }

    /// The end handle of an interpolation spline: (end point, grip).
    pub fn spline_handle_grip(
        &self,
        id: EntityId,
        end: bool,
    ) -> Option<(super::Point2, super::Point2)> {
        let n = self.spline_points(id).ok()?.len();
        self.spline_handle_grip_at(id, if end { n - 1 } else { 0 })
    }

    /// Sets the handle at point index `k` so that its grip lies at `grip`.
    pub fn set_spline_handle_grip_at(
        &mut self,
        id: EntityId,
        k: usize,
        grip: super::Point2,
    ) -> Result<(), SketchError> {
        let points = self.spline_points(id)?;
        let closed = self.interp.get(id).is_some_and(|f| f.closed);
        let fit: Vec<Vector2> = points
            .iter()
            .map(|&p| self.points.get(p).map(|q| q.position.coords))
            .collect::<Option<_>>()
            .ok_or(SketchError::UnknownPoint)?;
        if k >= fit.len() {
            return Err(SketchError::InvalidValue("no such spline point"));
        }
        let scale = Self::grip_scale(&fit, k, closed).max(1e-9);
        let h = (grip.coords - fit[k]) / scale;
        self.set_spline_handle_at(id, k, Some(h))
    }

    /// Sets an end handle so that its grip lies at `grip`.
    pub fn set_spline_handle_grip(
        &mut self,
        id: EntityId,
        end: bool,
        grip: super::Point2,
    ) -> Result<(), SketchError> {
        let n = self.spline_points(id)?.len();
        self.set_spline_handle_grip_at(id, if end { n - 1 } else { 0 }, grip)
    }

    /// **Close Curve** / open it again. Closing joins the last point to the
    /// first (the point list repeats it); needs ≥ 3 distinct points.
    /// Opening drops the repeat.
    pub fn set_spline_closed(&mut self, id: EntityId, closed: bool) -> Result<(), SketchError> {
        let was = self
            .interp
            .get(id)
            .ok_or(SketchError::InvalidValue("not an interpolation spline"))?
            .closed;
        if was == closed {
            return Ok(());
        }
        let Some(Entity::Spline { points }) = self.entities.get_mut(id) else {
            return Err(SketchError::UnknownEntity);
        };
        if closed {
            if points.len() < 3 {
                return Err(SketchError::InvalidValue(
                    "Close Curve needs at least three points",
                ));
            }
            let (first, last) = (points[0], points[points.len() - 1]);
            if first != last {
                points.push(first);
                // The old last point keeps its end handle as an inner one.
                if let Some(f) = self.interp.get_mut(id) {
                    let k = points.len() - 2;
                    if let Some(e) = f.end.take() {
                        f.inner.resize(k + 1, None);
                        f.inner[k] = Some(e);
                    }
                }
            }
        } else if points.len() > 3 && points[0] == points[points.len() - 1] {
            points.pop();
            if let Some(f) = self.interp.get_mut(id) {
                let k = points.len() - 1;
                if f.inner.len() > k {
                    f.end = f.inner[k].take();
                    f.inner.truncate(k);
                }
            }
        }
        if let Some(f) = self.interp.get_mut(id) {
            f.closed = closed;
        }
        Ok(())
    }

    /// **Convert** a spline between control-vertex and interpolation
    /// (Inventor's Convert to CV / Convert to Interpolation). The curve
    /// keeps its shape as far as the other kind allows: an interpolation
    /// spline becomes its control polygon (new points; the fit points go
    /// unless something else uses them), a CV spline passes through its
    /// own control points moved onto the curve.
    pub fn convert_spline(&mut self, id: EntityId) -> Result<(), SketchError> {
        let points = self.spline_points(id)?.clone();
        if let Some(data) = self.interp.get(id).cloned() {
            if data.closed {
                return Err(SketchError::InvalidValue("open the curve first"));
            }
            let (cv, _) = self.spline_def(id, &|p| self.points[p].position.coords);
            // Ends stay shared (other curves may be attached there).
            let mut new = vec![points[0]];
            for q in &cv[1..cv.len() - 1] {
                new.push(self.add_point(super::Point2::from(*q)));
            }
            new.push(points[points.len() - 1]);
            if let Some(Entity::Spline { points }) = self.entities.get_mut(id) {
                *points = new;
            }
            self.interp.remove(id);
            for p in &points[1..points.len() - 1] {
                // Constraints that tied a fit point to this spline go too.
                let doomed: Vec<_> = self
                    .constraints
                    .iter()
                    .filter(|(_, c)| c.references_point(*p) && c.references_entity(id))
                    .map(|(cid, _)| cid)
                    .collect();
                for c in doomed {
                    self.remove_constraint(c)?;
                }
                self.drop_orphan(*p)?;
            }
        } else {
            let c = self.spline_curve(id).ok_or(SketchError::UnknownEntity)?;
            let d = c.domain();
            let n = points.len();
            // Interior control points move to evenly spaced curve points.
            for (k, &p) in points.iter().enumerate().take(n - 1).skip(1) {
                let q = c.point_at(d.min + (d.max - d.min) * k as Real / (n - 1) as Real);
                if let Some(pt) = self.points.get_mut(p) {
                    pt.position = super::Point2::new(q.x, q.y);
                }
            }
            self.interp.insert(id, SplineFit::default());
        }
        Ok(())
    }

    /// Control points and knots (empty = clamped uniform) of a spline from
    /// point positions `pos` (the stored ones, or the solver's values).
    pub(crate) fn spline_def(
        &self,
        id: EntityId,
        pos: &dyn Fn(PointId) -> Vector2,
    ) -> (Vec<Vector2>, Vec<Real>) {
        let Some(Entity::Spline { points }) = self.entities.get(id) else {
            return (Vec::new(), Vec::new());
        };
        let fit: Vec<Vector2> = points.iter().map(|&p| pos(p)).collect();
        match self.interp.get(id) {
            None => (fit, Vec::new()),
            Some(f) => interpolate_fit(&fit, f),
        }
    }

    /// **Insert Point** on an interpolation spline at the curve point
    /// nearest to `at`; returns the new fit point.
    pub fn insert_spline_point(
        &mut self,
        id: EntityId,
        at: super::Point2,
    ) -> Result<PointId, SketchError> {
        if !self.is_interp_spline(id) {
            return Err(SketchError::InvalidValue("not an interpolation spline"));
        }
        let c = self.spline_curve(id).ok_or(SketchError::UnknownEntity)?;
        let n = match self.entities.get(id) {
            Some(Entity::Spline { points }) => points.len(),
            _ => return Err(SketchError::UnknownEntity),
        };
        // Nearest parameter by sampling.
        let d = c.domain();
        let (mut best, mut bu) = (Real::INFINITY, d.min);
        for k in 0..=400 {
            let u = d.min + (d.max - d.min) * k as Real / 400.0;
            let p = c.point_at(u);
            let dist = (super::Point2::new(p.x, p.y) - at).norm();
            if dist < best {
                (best, bu) = (dist, u);
            }
        }
        let t = (bu - d.min) / (d.max - d.min);
        let index = ((t * (n - 1) as Real).floor() as usize + 1).clamp(1, n - 1);
        let p = c.point_at(bu);
        let q = self.add_point(super::Point2::new(p.x, p.y));
        if let Some(Entity::Spline { points }) = self.entities.get_mut(id) {
            points.insert(index, q);
        }
        if let Some(f) = self.interp.get_mut(id)
            && f.inner.len() > index
        {
            f.inner.insert(index, None);
        }
        Ok(q)
    }

    /// **Delete Point** of an interpolation spline (keeps at least two);
    /// the point is removed when nothing else uses it.
    pub fn remove_spline_point(&mut self, id: EntityId, p: PointId) -> Result<(), SketchError> {
        if !self.is_interp_spline(id) {
            return Err(SketchError::InvalidValue("not an interpolation spline"));
        }
        let Some(Entity::Spline { points }) = self.entities.get_mut(id) else {
            return Err(SketchError::UnknownEntity);
        };
        if points.len() <= 2 {
            return Err(SketchError::InvalidValue(
                "a spline keeps at least two points",
            ));
        }
        let closed = self.interp.get(id).is_some_and(|f| f.closed);
        if closed && (points[0] == p || points.len() <= 4) {
            return Err(SketchError::InvalidValue(if points[0] == p {
                "the closing point of a closed curve stays"
            } else {
                "a closed curve keeps at least three points"
            }));
        }
        let k = points
            .iter()
            .position(|&q| q == p)
            .ok_or(SketchError::InvalidValue("not a point of this spline"))?;
        points.remove(k);
        if let Some(f) = self.interp.get_mut(id)
            && f.inner.len() > k
        {
            f.inner.remove(k);
        }
        // Constraints that tied this point to the spline go too.
        let doomed: Vec<_> = self
            .constraints
            .iter()
            .filter(|(_, c)| c.references_point(p) && c.references_entity(id))
            .map(|(cid, _)| cid)
            .collect();
        for c in doomed {
            self.remove_constraint(c)?;
        }
        self.drop_orphan(p)?;
        Ok(())
    }

    /// Curvature comb of a spline (Inventor's Display Curvature): `samples`
    /// pairs (point on the curve, point at `scale` × curvature along the
    /// normal).
    pub fn curvature_comb(
        &self,
        id: EntityId,
        samples: usize,
        scale: Real,
    ) -> Vec<(super::Point2, super::Point2)> {
        let Some(c) = self.spline_curve(id) else {
            return Vec::new();
        };
        let d = c.domain();
        let h = (d.max - d.min) * 1e-4;
        (0..=samples)
            .map(|k| {
                let u = d.min + (d.max - d.min) * k as Real / samples as Real;
                let p = c.point_at(u);
                let d1 = c.derivative_at(u);
                // Second derivative by a central difference of the first.
                let (ua, ub) = ((u - h).max(d.min), (u + h).min(d.max));
                let d2 = (c.derivative_at(ub) - c.derivative_at(ua)) / (ub - ua);
                let (dx, dy, ddx, ddy) = (d1.x, d1.y, d2.x, d2.y);
                let sp = (dx * dx + dy * dy).sqrt().max(1e-12);
                let kappa = (dx * ddy - dy * ddx) / (sp * sp * sp);
                let n = Vector2::new(-dy, dx) / sp;
                let base = super::Point2::new(p.x, p.y);
                (base, base + n * (-kappa * scale))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sketch::Point2;

    #[test]
    fn passes_through_its_points_with_natural_or_given_ends() {
        let fit = [
            Vector2::new(0.0, 0.0),
            Vector2::new(10.0, 8.0),
            Vector2::new(25.0, -3.0),
            Vector2::new(40.0, 5.0),
        ];
        let (cv, t) = interpolate(&fit, None, None);
        let pts: Vec<crate::kernel::Point3> = cv
            .iter()
            .map(|v| crate::kernel::Point3::new(v.x, v.y, 0.0))
            .collect();
        let c = crate::kernel::BSplineCurve::sketch_spline(pts, &t).unwrap();
        let u = chord_params(&fit);
        for (k, q) in fit.iter().enumerate() {
            let p = c.point_at(u[k]);
            assert!(
                (Vector2::new(p.x, p.y) - q).norm() < 1e-9,
                "through point {k}"
            );
        }
        // A handle sets the end tangent direction.
        let (cv, _) = interpolate(&fit, Some(Vector2::new(0.0, 3.0)), None);
        let t = cv[1] - cv[0];
        assert!(t.x.abs() < 1e-9 && t.y > 0.0, "{t:?}");
    }

    #[test]
    fn interp_spline_in_a_sketch_solves_and_follows_its_points() {
        let mut sk = Sketch::default();
        let pts: Vec<PointId> = [(0.0, 0.0), (10.0, 8.0), (25.0, -3.0)]
            .iter()
            .map(|&(x, y)| sk.add_point(Point2::new(x, y)))
            .collect();
        let s = sk.add_interp_spline(pts.clone()).unwrap();
        assert!(sk.is_interp_spline(s));
        // A point constrained onto the curve.
        let q = sk.add_point(Point2::new(12.0, 0.0));
        let c = sk.on_curve(q, s).unwrap();
        sk.add_constraint(c).unwrap();
        let rep = sk.solve().unwrap();
        assert_eq!(rep.redundant, 0);
        // The curve passes through the middle fit point.
        let curve = sk.spline_curve(s).unwrap();
        let p1 = sk.point(pts[1]).unwrap().position;
        let near = (0..=20000)
            .map(|k| {
                let q = curve.point_at(k as Real / 20000.0);
                (Point2::new(q.x, q.y) - p1).norm()
            })
            .fold(Real::INFINITY, Real::min);
        assert!(near < 1e-3, "{near}");
        // Grip API round trip.
        let (_, g) = sk.spline_handle_grip(s, false).unwrap();
        sk.set_spline_handle_grip(s, false, g + Vector2::new(0.0, 2.0))
            .unwrap();
        let (_, g2) = sk.spline_handle_grip(s, false).unwrap();
        assert!((g2 - (g + Vector2::new(0.0, 2.0))).norm() < 1e-9);
        // The handle stays when saved.
        sk.set_spline_handle(s, false, Some(Vector2::new(5.0, 5.0)))
            .unwrap();
        let json = serde_json::to_string(&sk).unwrap();
        let back: Sketch = serde_json::from_str(&json).unwrap();
        assert_eq!(back.spline_fit(s).unwrap().start, Some([5.0, 5.0]));
        // Insert / delete a fit point.
        let q = sk.insert_spline_point(s, Point2::new(18.0, 2.0)).unwrap();
        assert!(
            matches!(sk.entity(s), Some(Entity::Spline { points }) if points.len() == 4 && points[2] == q)
        );
        sk.remove_spline_point(s, q).unwrap();
        assert!(sk.point(q).is_none());
        // Curvature comb: as many teeth as asked.
        assert_eq!(sk.curvature_comb(s, 20, 10.0).len(), 21);
    }

    fn sampled(sk: &Sketch, s: EntityId) -> Vec<Vector2> {
        let c = sk.spline_curve(s).unwrap();
        let d = c.domain();
        (0..=50)
            .map(|k| {
                let p = c.point_at(d.min + (d.max - d.min) * k as Real / 50.0);
                Vector2::new(p.x, p.y)
            })
            .collect()
    }

    fn wave() -> (Sketch, Vec<PointId>, EntityId) {
        let mut sk = Sketch::default();
        let pts: Vec<PointId> = [(0.0, 0.0), (10.0, 8.0), (25.0, -3.0), (40.0, 5.0)]
            .iter()
            .map(|&(x, y)| sk.add_point(Point2::new(x, y)))
            .collect();
        let s = sk.add_interp_spline(pts.clone()).unwrap();
        (sk, pts, s)
    }

    #[test]
    fn interior_handle_keeps_the_shape_then_sets_the_tangent() {
        let (mut sk, pts, s) = wave();
        let before = sampled(&sk, s);
        sk.activate_spline_handle(s, 1).unwrap();
        let after = sampled(&sk, s);
        let dev = before
            .iter()
            .zip(&after)
            .map(|(a, b)| (a - b).norm())
            .fold(0.0, Real::max);
        assert!(dev < 1e-6, "activation moved the curve by {dev}");
        // A vertical handle: the curve leaves point 1 straight up.
        let (base, _) = sk.spline_handle_grip_at(s, 1).unwrap();
        sk.set_spline_handle_grip_at(s, 1, base + Vector2::new(0.0, 4.0))
            .unwrap();
        let (cv, t) = sk.spline_def(s, &|p| sk.point(p).unwrap().position.coords);
        let fit: Vec<Vector2> = pts
            .iter()
            .map(|&p| sk.point(p).unwrap().position.coords)
            .collect();
        let u = chord_params(&fit)[1];
        let d = derivative(&cv, &t, u, 1);
        assert!(d.x.abs() < 1e-9 && d.y > 0.0, "{d:?}");
        // Still through every point; grip round trip.
        let c = sk.spline_curve(s).unwrap();
        let p = c.point_at(u);
        assert!((Vector2::new(p.x, p.y) - fit[1]).norm() < 1e-9);
        let (_, g) = sk.spline_handle_grip_at(s, 1).unwrap();
        assert!((g - (base + Vector2::new(0.0, 4.0))).norm() < 1e-9);
        // Inserting a point before it shifts the handle along.
        sk.insert_spline_point(s, Point2::new(4.0, 4.0)).unwrap();
        assert!(sk.spline_fit(s).unwrap().handle(2, 5).is_some());
        sk.reset_spline_handles(s).unwrap();
        assert!(!sk.spline_fit(s).unwrap().has_handles());
    }

    #[test]
    fn closed_curve_is_periodic_and_bounds_a_region() {
        let (mut sk, _, s) = wave();
        sk.set_spline_closed(s, true).unwrap();
        let (cv, t) = sk.spline_def(s, &|p| sk.point(p).unwrap().position.coords);
        for k in [1, 2] {
            let (a, b) = (derivative(&cv, &t, 0.0, k), derivative(&cv, &t, 1.0, k));
            assert!(
                (a - b).norm() < 1e-6 * (1.0 + a.norm()),
                "C{k}: {a:?} {b:?}"
            );
        }
        assert_eq!(sk.solve().unwrap().redundant, 0);
        assert_eq!(sk.regions().len(), 1);
        // With an interior handle as well, and as the kernel curve evaluates.
        sk.activate_spline_handle(s, 1).unwrap();
        let (base, _) = sk.spline_handle_grip_at(s, 1).unwrap();
        sk.set_spline_handle_grip_at(s, 1, base + Vector2::new(3.0, 4.0))
            .unwrap();
        let c = sk.spline_curve(s).unwrap();
        let d = c.domain();
        let (a, b) = (c.derivative_at(d.min), c.derivative_at(d.max));
        assert!(
            (Vector2::new(a.x - b.x, a.y - b.y)).norm() < 1e-6 * (1.0 + a.norm()),
            "kernel C1: {a:?} {b:?}"
        );
        // Opening restores the open curve.
        sk.set_spline_closed(s, false).unwrap();
        assert!(matches!(sk.entity(s), Some(Entity::Spline { points }) if points.len() == 4));
        assert!(sk.regions().is_empty());
        // Too few points to close.
        let a = sk.add_point(Point2::new(0.0, 20.0));
        let b = sk.add_point(Point2::new(5.0, 20.0));
        let two = sk.add_interp_spline(vec![a, b]).unwrap();
        assert!(sk.set_spline_closed(two, true).is_err());
    }

    #[test]
    fn convert_between_cv_and_interpolation() {
        let (mut sk, pts, s) = wave();
        sk.convert_spline(s).unwrap();
        assert!(!sk.is_interp_spline(s));
        let Some(Entity::Spline { points }) = sk.entity(s) else {
            panic!()
        };
        // Control polygon: 4 fit points -> 6 control points, ends shared.
        assert_eq!(points.len(), 6);
        assert_eq!((points[0], points[5]), (pts[0], pts[3]));
        assert!(sk.point(pts[1]).is_none(), "old fit points dropped");
        // Back: passes through its (moved) points.
        sk.convert_spline(s).unwrap();
        assert!(sk.is_interp_spline(s));
        let Some(Entity::Spline { points }) = sk.entity(s).cloned() else {
            panic!()
        };
        let curve = sampled(&sk, s);
        for p in points {
            let q = sk.point(p).unwrap().position.coords;
            let near = curve
                .iter()
                .map(|c| (c - q).norm())
                .fold(Real::INFINITY, Real::min);
            assert!(near < 2.0, "{near}");
        }
    }
}
