//! Numerical constraint solver (Levenberg–Marquardt) and DOF analysis.
//!
//! ## Algorithm
//!
//! Let `x ∈ ℝⁿ` be the free DOFs and `F(x) ∈ ℝᵐ` the stacked residuals of all
//! constraints. We minimise `½‖F(x)‖²`:
//!
//! 1. Linearise: `F(x + δ) ≈ F(x) + J δ`, with `J = ∂F/∂x` (m × n).
//! 2. Solve the damped normal equations
//!    `(JᵀJ + λ·I) δ = −JᵀF`.
//! 3. If the step lowers the error, accept it and decrease `λ` (towards
//!    Gauss–Newton, fast quadratic convergence); otherwise increase `λ`
//!    (towards short gradient-descent steps, robust far from the solution).
//!
//! The damping also makes the method well behaved on **under-constrained**
//! sketches, where `JᵀJ` is singular. We deliberately use Levenberg's
//! *identity* damping rather than Marquardt's `λ·diag(JᵀJ)`: as `λ → 0` the
//! identity-damped step tends to the **minimum-norm** Gauss–Newton step, which
//! never moves along directions the constraints do not care about. With
//! diagonal scaling, weakly-coupled DOFs get almost no damping and can take
//! huge steps — e.g. the endpoints of a tangent line sliding kilometres along
//! the line. Minimum-norm steps mean "change the sketch as little as
//! possible", which is what a user dragging geometry expects.
//!
//! ## Jacobian
//!
//! `J` is computed by central finite differences, which is exact to ~1e-10
//! for these smooth residuals and keeps adding new constraint types trivial.
//! It is **sparse**: each equation (a constraint, or an arc's "end on the
//! circle") is differentiated only over the unknowns it reads (recorded once
//! per solve through [`Eval`]), so building `J` costs O(non-zeros) instead of
//! O(n·m) residual evaluations, and `JᵀJ` is accumulated row by row. A 10×10
//! sketch pattern of rectangles (800 unknowns) went from 450 ms to a few ms
//! per Jacobian.
//!
//! ## DOF analysis
//!
//! At the solution, `rank(J)` is the number of independent equations:
//! - `dof = n − rank` — remaining freedom (0 ⇒ fully constrained);
//! - `redundant = m − rank` — equations implied by the others
//!   (over-constrained, even if consistent).

use std::cell::RefCell;
use std::collections::HashMap;

use nalgebra::{DMatrix, DVector};
use serde::{Deserialize, Serialize};

use super::constraint::{Geo, arc_residual};
use super::{ConstraintId, Entity, EntityId, PointId, Sketch, SketchError, Vector2};
use crate::kernel::Real;

/// Tuning parameters for [`Sketch::solve_with`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SolverOptions {
    /// Maximum number of Levenberg–Marquardt iterations.
    pub max_iterations: usize,
    /// The sketch counts as solved when every residual is below this
    /// (mm for positional constraints, ≈ radians for angular ones).
    pub tolerance: Real,
}

impl Default for SolverOptions {
    fn default() -> Self {
        Self {
            max_iterations: 200,
            tolerance: 1e-10,
        }
    }
}

/// Outcome of a solve.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SolveStatus {
    /// All constraints are satisfied; the sketch was updated.
    Solved,
    /// The constraints could not be satisfied (contradictory, or no solution
    /// reachable from the current geometry); the sketch was left unchanged.
    Failed,
}

/// Diagnostics returned by every solve.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SolveReport {
    /// Solved or failed.
    pub status: SolveStatus,
    /// Iterations performed.
    pub iterations: usize,
    /// Largest absolute residual at the end.
    pub max_residual: Real,
    /// Remaining degrees of freedom.
    pub dof: usize,
    /// Number of redundant equations (over-constrained if > 0).
    pub redundant: usize,
}

impl SolveReport {
    /// Solved with no freedom left and nothing redundant — "fully defined".
    pub fn is_fully_constrained(&self) -> bool {
        self.status == SolveStatus::Solved && self.dof == 0 && self.redundant == 0
    }
}

/// One scalar unknown of the system.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Param {
    X(PointId),
    Y(PointId),
    Radius(EntityId),
}

/// Read-only view of the sketch with candidate DOF values substituted in.
/// Residual functions read geometry exclusively through this.
pub(crate) struct Eval<'a> {
    sketch: &'a Sketch,
    index: &'a HashMap<Param, usize>,
    x: &'a DVector<Real>,
    /// Records the unknowns read (sparsity of the Jacobian).
    log: Option<&'a RefCell<Vec<usize>>>,
}

impl Eval<'_> {
    fn value(&self, p: Param, stored: Real) -> Real {
        match self.index.get(&p) {
            Some(&i) => {
                if let Some(l) = self.log {
                    l.borrow_mut().push(i);
                }
                self.x[i]
            }
            None => stored,
        }
    }
}

/// One equation group of the system, evaluated on its own.
#[derive(Clone, Copy)]
enum Equation {
    Constraint(ConstraintId),
    /// An arc's end lies on the circle through its start.
    Arc(EntityId),
}

/// Sparse Jacobian: per row, the non-zero (column, value) entries.
type SparseRows = Vec<Vec<(usize, Real)>>;

impl Geo for Eval<'_> {
    fn sketch(&self) -> &Sketch {
        self.sketch
    }

    /// Position of a point (as a vector from the sketch origin).
    fn point(&self, id: PointId) -> Vector2 {
        let stored = self.sketch.points[id].position;
        Vector2::new(
            self.value(Param::X(id), stored.x),
            self.value(Param::Y(id), stored.y),
        )
    }

    fn radius_param(&self, id: EntityId, stored: Real) -> Real {
        self.value(Param::Radius(id), stored)
    }
}

/// The nonlinear system `F(x) = 0` built from a sketch.
struct System<'a> {
    sketch: &'a Sketch,
    params: Vec<Param>,
    index: HashMap<Param, usize>,
    equations: Vec<Equation>,
}

impl<'a> System<'a> {
    fn new(sketch: &'a Sketch) -> Self {
        let mut params = Vec::new();
        for (id, p) in &sketch.points {
            if !p.fixed {
                params.extend([Param::X(id), Param::Y(id)]);
            }
        }
        for (id, e) in &sketch.entities {
            if matches!(e, Entity::Circle { .. }) {
                params.push(Param::Radius(id));
            }
        }
        let index = params.iter().enumerate().map(|(i, &p)| (p, i)).collect();
        let mut equations: Vec<Equation> = sketch
            .constraints
            .keys()
            .filter(|id| !sketch.driven.contains_key(*id))
            .map(Equation::Constraint)
            .collect();
        // Two arcs on the same centre and end points (e.g. the halves of a
        // circle mirrored about its diameter) share one circle equation.
        let mut seen = std::collections::HashSet::new();
        for (id, e) in &sketch.entities {
            if let Entity::Arc { center, start, end } = *e {
                if seen.contains(&(center, end, start)) || !seen.insert((center, start, end)) {
                    continue;
                }
                equations.push(Equation::Arc(id));
            }
        }
        Self {
            sketch,
            params,
            index,
            equations,
        }
    }

    /// Appends the residuals of one equation at `x` to `out`.
    fn eval_equation(
        &self,
        eq: Equation,
        x: &DVector<Real>,
        log: Option<&RefCell<Vec<usize>>>,
        out: &mut Vec<Real>,
    ) {
        let ev = Eval {
            sketch: self.sketch,
            index: &self.index,
            x,
            log,
        };
        match eq {
            Equation::Constraint(id) => {
                if let Some(c) = self.sketch.constraints.get(id) {
                    c.residuals(&ev, out);
                }
            }
            Equation::Arc(id) => {
                if let Some(e) = self.sketch.entities.get(id) {
                    arc_residual(&ev, e, out);
                }
            }
        }
    }

    /// Current DOF values read from the sketch.
    fn initial_values(&self) -> DVector<Real> {
        DVector::from_iterator(
            self.params.len(),
            self.params.iter().map(|p| match *p {
                Param::X(id) => self.sketch.points[id].position.x,
                Param::Y(id) => self.sketch.points[id].position.y,
                Param::Radius(id) => match self.sketch.entities[id] {
                    Entity::Circle { radius, .. } => radius,
                    // Only circles get a radius DOF.
                    _ => 0.0,
                },
            }),
        )
    }

    /// Evaluates `F(x)`.
    fn residuals(&self, x: &DVector<Real>) -> DVector<Real> {
        let mut out = Vec::new();
        for &eq in &self.equations {
            self.eval_equation(eq, x, None, &mut out);
        }
        DVector::from_vec(out)
    }

    /// `J = ∂F/∂x` by central differences, row by row over the unknowns each
    /// equation reads (`m` rows).
    fn jacobian(&self, x: &DVector<Real>, m: usize) -> SparseRows {
        let mut rows: SparseRows = Vec::with_capacity(m);
        let mut xp = x.clone();
        let mut base = Vec::new();
        let (mut fp, mut fm) = (Vec::new(), Vec::new());
        for &eq in &self.equations {
            let log = RefCell::new(Vec::new());
            base.clear();
            self.eval_equation(eq, x, Some(&log), &mut base);
            let mut cols = log.into_inner();
            cols.sort_unstable();
            cols.dedup();
            let first = rows.len();
            rows.extend((0..base.len()).map(|_| Vec::with_capacity(cols.len())));
            for &k in &cols {
                let h = 1e-6 * (1.0 + x[k].abs());
                fp.clear();
                fm.clear();
                xp[k] = x[k] + h;
                self.eval_equation(eq, &xp, None, &mut fp);
                xp[k] = x[k] - h;
                self.eval_equation(eq, &xp, None, &mut fm);
                xp[k] = x[k];
                for (r, (a, b)) in fp.iter().zip(&fm).enumerate() {
                    let d = (a - b) / (2.0 * h);
                    if d != 0.0
                        && let Some(row) = rows.get_mut(first + r)
                    {
                        row.push((k, d));
                    }
                }
            }
        }
        rows.truncate(m);
        rows
    }

    /// Writes DOF values back into the sketch.
    fn apply(params: &[Param], x: &DVector<Real>, sketch: &mut Sketch) {
        for (&p, &v) in params.iter().zip(x.iter()) {
            match p {
                Param::X(id) => sketch.points[id].position.x = v,
                Param::Y(id) => sketch.points[id].position.y = v,
                Param::Radius(id) => {
                    if let Entity::Circle { radius, .. } = &mut sketch.entities[id] {
                        *radius = v;
                    }
                }
            }
        }
    }
}

/// Above this many unknowns the solver steps with a sparse factorisation.
const DENSE_MAX: usize = 150;

/// `Jᵀr` of a sparse Jacobian.
fn sparse_jt(j: &SparseRows, r: &DVector<Real>, n: usize) -> DVector<Real> {
    let mut g = DVector::zeros(n);
    for (row, &ri) in j.iter().zip(r.iter()) {
        for &(k, v) in row {
            g[k] += v * ri;
        }
    }
    g
}

/// Squared column norms of a sparse Jacobian (the diagonal of `JᵀJ`).
fn column_norms2(j: &SparseRows, n: usize) -> DVector<Real> {
    let mut d = DVector::zeros(n);
    for row in j {
        for &(k, v) in row {
            d[k] += v * v;
        }
    }
    d
}

/// Solves `(JᵀJ + damping·I) δ = b` directly by a sparse LDLᵀ
/// factorisation, eliminating unknowns in minimum-degree order (the
/// unknown with the fewest neighbours first keeps the fill-in small; a
/// pattern's chain of copies factors almost like a banded matrix). The
/// damped matrix is positive definite, so every pivot is positive.
/// Conjugate gradients needed ~500 iterations here: the matrix of an
/// under-constrained sketch is nearly singular.
fn sparse_ldl_solve(j: &SparseRows, n: usize, damping: Real, b: &DVector<Real>) -> DVector<Real> {
    use std::collections::HashMap;
    // A = JᵀJ + damping·I, symmetric, as one map per row (both halves).
    let mut a: Vec<HashMap<usize, Real>> = vec![HashMap::new(); n];
    for (i, row) in a.iter_mut().enumerate() {
        row.insert(i, damping);
    }
    for row in j {
        for &(p, vp) in row {
            for &(q, vq) in row {
                *a[p].entry(q).or_insert(0.0) += vp * vq;
            }
        }
    }
    // Elimination: L columns (below the pivot, in elimination order).
    let mut done = vec![false; n];
    let mut order = Vec::with_capacity(n);
    let mut d = vec![0.0; n];
    let mut l: Vec<Vec<(usize, Real)>> = vec![Vec::new(); n];
    for _ in 0..n {
        let Some(v) = (0..n).filter(|&i| !done[i]).min_by_key(|&i| a[i].len()) else {
            break;
        };
        done[v] = true;
        order.push(v);
        let row = std::mem::take(&mut a[v]);
        let pivot = row.get(&v).copied().unwrap_or(damping).max(1e-300);
        d[v] = pivot;
        let others: Vec<(usize, Real)> = row.into_iter().filter(|&(k, _)| k != v).collect();
        for &(u, auv) in &others {
            a[u].remove(&v);
            for &(w, awv) in &others {
                *a[u].entry(w).or_insert(0.0) -= auv * awv / pivot;
            }
        }
        l[v] = others.into_iter().map(|(k, x)| (k, x / pivot)).collect();
    }
    // Solve L D Lᵀ x = b in elimination order.
    let mut y = b.clone();
    for &v in &order {
        let yv = y[v];
        for &(k, lk) in &l[v] {
            y[k] -= lk * yv;
        }
    }
    for &v in &order {
        y[v] /= d[v];
    }
    for &v in order.iter().rev() {
        let s: Real = l[v].iter().map(|&(k, lk)| lk * y[k]).sum();
        y[v] -= s;
    }
    y
}

/// `JᵀJ` and `Jᵀr` of a sparse Jacobian (`n` columns).
fn normal_equations(j: &SparseRows, r: &DVector<Real>, n: usize) -> (DMatrix<Real>, DVector<Real>) {
    let mut a = DMatrix::zeros(n, n);
    let mut g = DVector::zeros(n);
    for (row, &ri) in j.iter().zip(r.iter()) {
        for &(p, vp) in row {
            g[p] += vp * ri;
            for &(q, vq) in row {
                a[(p, q)] += vp * vq;
            }
        }
    }
    (a, g)
}

/// The dense m × `n` matrix of a sparse Jacobian.
#[cfg(test)]
fn dense(j: &SparseRows, n: usize) -> DMatrix<Real> {
    let mut d = DMatrix::zeros(j.len(), n);
    for (i, row) in j.iter().enumerate() {
        for &(k, v) in row {
            d[(i, k)] += v;
        }
    }
    d
}

/// Numerical rank of a sparse Jacobian with `n` columns: Gaussian
/// elimination with partial pivoting on unit-norm rows, column by column;
/// a column whose best pivot is below `1e-8` is dependent. The rows of a
/// sketch Jacobian have a handful of entries and neighbouring unknowns
/// belong to the same geometry, so fill-in stays small: O(non-zeros) in
/// practice instead of the SVD's O(n³) (800 unknowns: 370 ms → ~1 ms).
fn sparse_rank(j: &SparseRows, n: usize) -> usize {
    Echelon::new(j, n).rank
}

/// A pivot of [`Echelon`]: its value and its (sparse) row.
type Pivot = (Real, Vec<(usize, Real)>);

/// Row echelon form of a sparse Jacobian (see [`sparse_rank`]): per column
/// its pivot row, if the column is independent.
struct Echelon {
    n: usize,
    /// Per column: the pivot value and the pivot row.
    pivots: Vec<Option<Pivot>>,
    rank: usize,
}

impl Echelon {
    fn new(j: &SparseRows, n: usize) -> Self {
        const TOL: Real = 1e-8;
        let mut rows: Vec<Vec<(usize, Real)>> = j
            .iter()
            .filter_map(|row| {
                let mut r = row.clone();
                r.sort_unstable_by_key(|e| e.0);
                r.dedup_by(|b, a| {
                    if a.0 == b.0 {
                        a.1 += b.1;
                        true
                    } else {
                        false
                    }
                });
                let norm = r.iter().map(|e| e.1 * e.1).sum::<Real>().sqrt();
                (norm > 0.0).then(|| r.into_iter().map(|(k, v)| (k, v / norm)).collect())
            })
            .collect();
        // Rows that (may) hold each column; stale entries are re-checked.
        let mut by_col: Vec<Vec<usize>> = vec![Vec::new(); n];
        for (i, row) in rows.iter().enumerate() {
            for &(k, _) in row {
                if let Some(c) = by_col.get_mut(k) {
                    c.push(i);
                }
            }
        }
        let mut used = vec![false; rows.len()];
        let value = |row: &[(usize, Real)], c: usize| {
            row.binary_search_by_key(&c, |e| e.0)
                .ok()
                .map_or(0.0, |k| row[k].1)
        };
        let mut rank = 0;
        let mut pivots = vec![None; n];
        for c in 0..n {
            let mut cand = std::mem::take(&mut by_col[c]);
            cand.sort_unstable();
            cand.dedup();
            cand.retain(|&i| !used[i] && value(&rows[i], c) != 0.0);
            let Some(&p) = cand.iter().max_by(|&&a, &&b| {
                value(&rows[a], c)
                    .abs()
                    .total_cmp(&value(&rows[b], c).abs())
            }) else {
                continue;
            };
            let pv = value(&rows[p], c);
            if pv.abs() <= TOL {
                continue;
            }
            used[p] = true;
            rank += 1;
            let pivot = rows[p].clone();
            pivots[c] = Some((pv, pivot.clone()));
            for &i in cand.iter().filter(|&&i| i != p) {
                let f = value(&rows[i], c) / pv;
                // rows[i] -= f · pivot (sorted merge), dropping column c.
                let old = std::mem::take(&mut rows[i]);
                let mut out = Vec::with_capacity(old.len() + pivot.len());
                let (mut a, mut b) = (0, 0);
                while a < old.len() || b < pivot.len() {
                    let (k, v) = match (old.get(a), pivot.get(b)) {
                        (Some(&(ka, va)), Some(&(kb, vb))) if ka == kb => {
                            a += 1;
                            b += 1;
                            (ka, va - f * vb)
                        }
                        (Some(&(ka, va)), Some(&(kb, _))) if ka < kb => {
                            a += 1;
                            (ka, va)
                        }
                        (Some(&(ka, va)), None) => {
                            a += 1;
                            (ka, va)
                        }
                        (_, Some(&(kb, vb))) => {
                            b += 1;
                            if !old.iter().any(|e| e.0 == kb)
                                && let Some(col) = by_col.get_mut(kb)
                            {
                                col.push(i);
                            }
                            (kb, -f * vb)
                        }
                        (None, None) => break,
                    };
                    if k != c && v.abs() > 1e-14 {
                        out.push((k, v));
                    }
                }
                rows[i] = out;
            }
        }
        Self { n, pivots, rank }
    }

    /// What is left of the linear functional `v` (dense, one entry per
    /// unknown) after removing the row space of `J`: (near) zero iff `v` is
    /// a combination of the equations, i.e. the constraints determine it.
    fn reduce(&self, mut v: Vec<Real>) -> Vec<Real> {
        v.resize(self.n, 0.0);
        for c in 0..self.n {
            if v[c] == 0.0 {
                continue;
            }
            if let Some((pv, row)) = &self.pivots[c] {
                let f = v[c] / pv;
                for &(k, x) in row {
                    v[k] -= f * x;
                }
                v[c] = 0.0;
            }
        }
        v
    }
}

/// Numerical rank of `j`: singular values above a relative threshold.
#[cfg(test)]
fn rank(j: &DMatrix<Real>) -> usize {
    if j.is_empty() {
        return 0;
    }
    let sv = j.clone().svd(false, false).singular_values;
    let max = sv.max();
    if max <= 0.0 {
        return 0;
    }
    sv.iter().filter(|&&s| s > max * 1e-8).count()
}

/// Solves `sketch` in place (see the module docs for the algorithm).
pub(crate) fn solve(sketch: &mut Sketch, opts: &SolverOptions) -> Result<SolveReport, SketchError> {
    for c in sketch.constraints.values() {
        c.validate(sketch)?;
    }

    let system = System::new(sketch);
    let n = system.params.len();
    let mut x = system.initial_values();
    let mut r = system.residuals(&x);
    let m = r.len();
    let mut cost = r.norm_squared();
    // λ is relative to the largest diagonal entry of JᵀJ, so the damping
    // means the same thing for millimetre- and radian-valued residuals.
    // Large sketches start nearly undamped (Gauss–Newton): long chains of
    // constraints (pattern copies tied one to the next) have tiny singular
    // values that a 1e-3 damping would crawl along for a dozen iterations.
    // A failing step still raises λ.
    let mut mu: Real = if n > DENSE_MAX { 1e-9 } else { 1e-3 };
    let mut iterations = 0;

    while n > 0 && r.amax() > opts.tolerance && iterations < opts.max_iterations {
        iterations += 1;
        let j = system.jacobian(&x, m);
        // Large sketches (patterns): a sparse LDLᵀ factorisation instead of
        // a dense O(n³) Cholesky one.
        let large = n > DENSE_MAX;
        let (a, g) = if large {
            (DMatrix::zeros(0, 0), sparse_jt(&j, &r, n))
        } else {
            normal_equations(&j, &r, n)
        };

        // Inner loop: raise λ until a step actually reduces the error.
        let scale = if large {
            column_norms2(&j, n).max().max(1e-12)
        } else {
            a.diagonal().max().max(1e-12)
        };
        let mut step = None;
        while mu < 1e12 {
            let delta = if large {
                Some(sparse_ldl_solve(&j, n, mu * scale, &(-&g)))
            } else {
                let mut damped = a.clone();
                for i in 0..n {
                    damped[(i, i)] += mu * scale;
                }
                damped.cholesky().map(|chol| chol.solve(&(-&g)))
            };
            if let Some(delta) = delta {
                let x_new = &x + &delta;
                let r_new = system.residuals(&x_new);
                let cost_new = r_new.norm_squared();
                if cost_new < cost {
                    step = Some((x_new, r_new, cost_new, delta.norm()));
                    break;
                }
            }
            mu *= 4.0;
        }

        let Some((x_new, r_new, cost_new, step_len)) = step else {
            break; // Stalled: no descent direction left (local minimum ≠ 0).
        };
        x = x_new;
        r = r_new;
        cost = cost_new;
        mu = (mu / 3.0).max(1e-10);
        if step_len <= 1e-15 * (1.0 + x.norm()) {
            break;
        }
    }

    let max_residual = if m == 0 { 0.0 } else { r.amax() };
    let status = if max_residual <= opts.tolerance {
        SolveStatus::Solved
    } else {
        SolveStatus::Failed
    };
    let rank = if m == 0 || n == 0 {
        0
    } else {
        let j = system.jacobian(&x, m);
        let r = sparse_rank(&j, n);
        #[cfg(test)]
        {
            let dense_rank = rank(&dense(&j, n));
            assert_eq!(r, dense_rank, "sparse rank differs from the SVD rank");
        }
        r
    };
    let report = SolveReport {
        status,
        iterations,
        max_residual,
        dof: n - rank,
        redundant: m - rank,
    };

    if status == SolveStatus::Solved {
        let params = system.params;
        System::apply(&params, &x, sketch);
    }
    Ok(report)
}

/// How a point may still move (Inventor's degrees-of-freedom glyphs).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PointFreedom {
    /// Fully determined.
    Fixed,
    /// Can slide along one direction (unit vector, sketch coordinates).
    Along(Vector2),
    /// Free to move anywhere in the plane.
    Free,
}

/// Per-geometry degrees of freedom of a sketch.
#[derive(Clone, Debug, Default)]
pub struct DofAnalysis {
    /// Freedom of every point.
    pub points: HashMap<PointId, PointFreedom>,
    /// Curves that can still rotate (their direction is not determined).
    pub rotating: Vec<EntityId>,
    /// Circles whose radius is not determined.
    pub free_radius: Vec<EntityId>,
    /// Entities with no freedom left at all (drawn as fully constrained).
    pub fully_constrained: Vec<EntityId>,
}

/// Analyses which geometry can still move, from the null space of the
/// constraint Jacobian at the current geometry: every null vector is a
/// motion that keeps all constraints satisfied to first order.
///
/// `JᵀJ` is eigen-decomposed (its null space equals `J`'s and is complete
/// even when there are fewer equations than unknowns).
pub(crate) fn dof_analysis(sketch: &Sketch) -> DofAnalysis {
    let system = System::new(sketch);
    let n = system.params.len();
    let x = system.initial_values();
    let m = system.residuals(&x).len();
    let ech = Echelon::new(&system.jacobian(&x, m), n);
    let eps = 1e-6;
    let unit = |p: Param| {
        let mut v = vec![0.0; n];
        if let Some(&i) = system.index.get(&p) {
            v[i] = 1.0;
        }
        v
    };
    let dot = |a: &[Real], b: &[Real]| a.iter().zip(b).map(|(x, y)| x * y).sum::<Real>();

    let mut out = DofAnalysis::default();
    for (id, _) in &sketch.points {
        // Unconstrained parts of x and y: the point is fixed when both are
        // determined, slides when one combination is.
        let rx = ech.reduce(unit(Param::X(id)));
        let ry = ech.reduce(unit(Param::Y(id)));
        let (a, b, c) = (dot(&rx, &rx), dot(&rx, &ry), dot(&ry, &ry));
        // Eigenvalues of the 2 × 2 Gram matrix = squared singular values.
        let mean = (a + c) / 2.0;
        let disc = (((a - c) / 2.0).powi(2) + b * b).sqrt();
        let (big, small) = (mean + disc, (mean - disc).max(0.0));
        let freedom = if big.sqrt() <= eps {
            PointFreedom::Fixed
        } else if small.sqrt() > eps {
            PointFreedom::Free
        } else {
            // Motion along the eigenvector of the large eigenvalue.
            let u = if b.abs() > 1e-300 {
                Vector2::new(big - c, b)
            } else if a >= c {
                Vector2::new(1.0, 0.0)
            } else {
                Vector2::new(0.0, 1.0)
            };
            PointFreedom::Along(u.normalize())
        };
        out.points.insert(id, freedom);
    }

    for (id, e) in &sketch.entities {
        let mut free = e
            .points()
            .iter()
            .any(|p| out.points.get(p) != Some(&PointFreedom::Fixed));
        match e {
            Entity::Line { start, end } => {
                // Angular velocity of the line as a functional of the motion.
                let (a, b) = (sketch.points[*start].position, sketch.points[*end].position);
                let d = b - a;
                let l2 = d.norm_squared().max(1e-12);
                let mut w = vec![0.0; n];
                let mut set = |p: Param, v: Real| {
                    if let Some(&i) = system.index.get(&p) {
                        w[i] += v;
                    }
                };
                set(Param::X(*end), -d.y / l2);
                set(Param::Y(*end), d.x / l2);
                set(Param::X(*start), d.y / l2);
                set(Param::Y(*start), -d.x / l2);
                let r = ech.reduce(w);
                if dot(&r, &r).sqrt() > eps {
                    out.rotating.push(id);
                }
            }
            Entity::Circle { .. } => {
                let r = ech.reduce(unit(Param::Radius(id)));
                if system.index.contains_key(&Param::Radius(id)) && dot(&r, &r).sqrt() > eps {
                    out.free_radius.push(id);
                    free = true;
                }
            }
            _ => {}
        }
        if !free {
            out.fully_constrained.push(id);
        }
    }
    #[cfg(test)]
    check_against_eigen(sketch, &out);
    out
}

/// Tests: the elimination-based analysis must agree with the
/// eigen-decomposition of `JᵀJ` (the previous, O(n³) method).
#[cfg(test)]
fn check_against_eigen(sketch: &Sketch, fast: &DofAnalysis) {
    let slow = dof_analysis_eigen(sketch);
    for (id, f) in &fast.points {
        let g = slow.points.get(id);
        let same = match (f, g) {
            (PointFreedom::Along(u), Some(PointFreedom::Along(v))) => u.perp(v).abs() < 1e-6,
            (a, Some(b)) => a == b,
            _ => false,
        };
        assert!(same, "point freedom {f:?} vs eigen {g:?}");
    }
    let sorted = |v: &[EntityId]| {
        let mut v = v.to_vec();
        v.sort();
        v
    };
    assert_eq!(sorted(&fast.rotating), sorted(&slow.rotating), "rotating");
    assert_eq!(
        sorted(&fast.free_radius),
        sorted(&slow.free_radius),
        "free radius"
    );
    assert_eq!(
        sorted(&fast.fully_constrained),
        sorted(&slow.fully_constrained),
        "fully constrained"
    );
}

#[cfg(test)]
fn dof_analysis_eigen(sketch: &Sketch) -> DofAnalysis {
    let system = System::new(sketch);
    let n = system.params.len();
    let x = system.initial_values();
    let m = system.residuals(&x).len();
    let null: DMatrix<Real> = if n == 0 {
        DMatrix::zeros(0, 0)
    } else if m == 0 {
        DMatrix::identity(n, n)
    } else {
        let j = system.jacobian(&x, m);
        let (jtj, _) = normal_equations(&j, &DVector::zeros(j.len()), n);
        let eig = jtj.symmetric_eigen();
        let max = eig.eigenvalues.amax().max(1e-30);
        // Singular value σ = √λ; null if σ < 1e-7·σ_max.
        let cols: Vec<_> = (0..n)
            .filter(|&k| eig.eigenvalues[k] < max * 1e-14)
            .map(|k| eig.eigenvectors.column(k).into_owned())
            .collect();
        if cols.is_empty() {
            DMatrix::zeros(n, 0)
        } else {
            DMatrix::from_columns(&cols)
        }
    };
    let k = null.ncols();
    let row = |p: Param| system.index.get(&p).map(|&i| null.row(i).transpose());
    let zero = DVector::zeros(k);
    let eps = 1e-6;

    // Motion of a point across the null basis: 2 × k.
    let motion = |id: PointId| -> DMatrix<Real> {
        let (rx, ry) = (
            row(Param::X(id)).unwrap_or(zero.clone()),
            row(Param::Y(id)).unwrap_or(zero.clone()),
        );
        DMatrix::from_rows(&[rx.transpose(), ry.transpose()])
    };

    let mut out = DofAnalysis::default();
    for (id, _) in &sketch.points {
        let mm = motion(id);
        let freedom = if k == 0 {
            PointFreedom::Fixed
        } else {
            let svd = mm.clone().svd(true, false);
            let s = &svd.singular_values;
            let r = s.iter().filter(|&&v| v > eps).count();
            match r {
                0 => PointFreedom::Fixed,
                // `u` was requested from the SVD, so it is present.
                1 => match svd.u.as_ref() {
                    Some(u) => {
                        let u = u.column(s.imax()).into_owned();
                        PointFreedom::Along(Vector2::new(u[0], u[1]).normalize())
                    }
                    None => PointFreedom::Free,
                },
                _ => PointFreedom::Free,
            }
        };
        out.points.insert(id, freedom);
    }

    for (id, e) in &sketch.entities {
        let mut free = e
            .points()
            .iter()
            .any(|p| out.points.get(p) != Some(&PointFreedom::Fixed));
        match e {
            Entity::Line { start, end } if k > 0 => {
                // Angular velocity of the line in each null direction.
                let (a, b) = (sketch.points[*start].position, sketch.points[*end].position);
                let d = b - a;
                let (ma, mb) = (motion(*start), motion(*end));
                let w = (0..k).map(|c| {
                    let dv = Vector2::new(mb[(0, c)] - ma[(0, c)], mb[(1, c)] - ma[(1, c)]);
                    d.perp(&dv) / d.norm_squared().max(1e-12)
                });
                if w.map(|v| v * v).sum::<Real>().sqrt() > eps {
                    out.rotating.push(id);
                }
            }
            Entity::Circle { .. } if row(Param::Radius(id)).is_some_and(|r| r.norm() > eps) => {
                out.free_radius.push(id);
                free = true;
            }
            _ => {}
        }
        if !free {
            out.fully_constrained.push(id);
        }
    }
    out
}

#[cfg(test)]
mod elimination_checks {
    use crate::sketch::{Constraint, Point2, Sketch};

    /// Every analysis compares itself with the eigen method
    /// (`check_against_eigen`): run it over shapes with free, sliding,
    /// rotating and fixed geometry, before and after dimensioning.
    #[test]
    fn dof_analysis_matches_eigen_on_assorted_sketches() {
        let mut sk = Sketch::default();
        let p = Point2::new;
        sk.add_rectangle_two_point(p(0.0, 0.0), p(10.0, 5.0))
            .unwrap();
        sk.add_rectangle_three_point(p(20.0, 0.0), p(30.0, 3.0), p(28.0, 9.0))
            .unwrap();
        sk.add_slot_center_to_center(p(0.0, 20.0), p(15.0, 22.0), 4.0)
            .unwrap();
        sk.add_arc_slot(p(40.0, 0.0), p(50.0, 0.0), p(40.0, 10.0), 2.0)
            .unwrap();
        sk.add_polygon(p(60.0, 20.0), p(65.0, 20.0), 6, true)
            .unwrap();
        let c = sk.add_circle(p(-20.0, 0.0), 3.0).unwrap();
        let l = sk.add_line(p(-30.0, -10.0), p(-10.0, -12.0));
        sk.solve().unwrap();
        let _ = sk.dof_analysis();
        // Fix parts: a fixed point, a horizontal line, a radius.
        let a = sk.add_fixed_point(p(-30.0, -10.0));
        let ends = sk.entity(l).map(|e| e.endpoints()).unwrap();
        sk.add_constraint(Constraint::Coincident { a, b: ends[0] })
            .unwrap();
        sk.add_constraint(Constraint::Horizontal { line: l })
            .unwrap();
        sk.add_constraint(Constraint::Radius {
            circle: c,
            value: 3.0,
        })
        .unwrap();
        sk.solve().unwrap();
        let _ = sk.dof_analysis();
        // A circular pattern of the slot's lines around a fixed centre.
        let centre = sk.add_fixed_point(p(0.0, 60.0));
        let line = sk.add_line(p(0.0, 62.0), p(0.0, 70.0));
        sk.circular_pattern(&[line], centre, 6, std::f64::consts::TAU, false)
            .unwrap();
        sk.solve().unwrap();
        let _ = sk.dof_analysis();
    }
}
