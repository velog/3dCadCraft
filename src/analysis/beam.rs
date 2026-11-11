//! The linear solver of Frame Analysis (`docs/spec/7.1a-frame-analysis.md`),
//! GUI-free and independent of assemblies: straight prismatic Euler-Bernoulli
//! beams in space between nodes with six degrees of freedom each.
//!
//! Units: mm, N, N·mm, MPa, t/mm³ for density (so that N = t·mm/s²).
//!
//! A beam's local axes are `x` along it (start to end), and its section's
//! horizontal and vertical axes `u`, `v` with `u × v = x`. Loads inside a
//! beam split it at their stations ([`Model::mesh`]); between two stations the
//! only span load is a uniform one, for which the end displacements and the
//! particular solution give the exact curve, so results do not depend on how
//! finely a beam is divided (static analysis).

use nalgebra::{DMatrix, DVector, Matrix3, SMatrix, Vector3};
use std::collections::BTreeSet;

/// Numbers.
pub type Real = f64;
/// A vector in the model's coordinates.
pub type V3 = Vector3<Real>;

/// One beam's section numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BeamSection {
    /// Area, mm².
    pub area: Real,
    /// Second moment about the section's horizontal axis `u`, mm⁴.
    pub iu: Real,
    /// Second moment about its vertical axis `v`, mm⁴.
    pub iv: Real,
    /// Torsion constant, mm⁴.
    pub j: Real,
    /// Elastic modulus about `u` (`iu` / farthest fibre), mm³.
    pub wu: Real,
    /// Elastic modulus about `v`, mm³.
    pub wv: Real,
    /// Torsional modulus, mm³.
    pub wt: Real,
    /// Shear area along `u`, mm².
    pub au: Real,
    /// Shear area along `v`, mm².
    pub av: Real,
}

/// One beam's material.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BeamMaterial {
    /// Young's modulus, MPa.
    pub e: Real,
    /// Shear modulus, MPa.
    pub g: Real,
    /// Density, t/mm³.
    pub rho: Real,
}

/// Six switches of a node or a beam end: translations x y z, rotations x y z
/// (a beam end's are in its local axes `x u v`).
pub type Dofs = [bool; 6];

/// A beam between two points of the model.
#[derive(Clone, Debug, PartialEq)]
pub struct Beam {
    /// Start node.
    pub start: usize,
    /// End node.
    pub end: usize,
    /// The section's horizontal axis `u` in the model (unit, square to the
    /// beam).
    pub u: V3,
    /// Section.
    pub section: BeamSection,
    /// Material.
    pub material: BeamMaterial,
    /// What the start end does **not** carry (released).
    pub release_start: Dofs,
    /// What the far end does not carry.
    pub release_end: Dofs,
}

/// A load on the model.
#[derive(Clone, Debug, PartialEq)]
pub enum Load {
    /// Force (N) and moment (N·mm) at a node.
    Node {
        /// The node.
        node: usize,
        /// Force.
        force: V3,
        /// Moment.
        moment: V3,
    },
    /// Force and moment at distance `at` (mm) from a beam's start.
    Point {
        /// The beam.
        beam: usize,
        /// Distance from its start.
        at: Real,
        /// Force.
        force: V3,
        /// Moment.
        moment: V3,
    },
    /// A uniform load (N/mm) over `from`..`to` (mm from the start).
    Uniform {
        /// The beam.
        beam: usize,
        /// Where it starts.
        from: Real,
        /// Where it ends.
        to: Real,
        /// Load per mm.
        q: V3,
    },
    /// Weight: acceleration (mm/s²) on every beam.
    Gravity {
        /// The acceleration.
        g: V3,
    },
}

/// A rigid connection: `child` follows `parent` as one rigid body.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RigidLink {
    /// The node that keeps its unknowns.
    pub parent: usize,
    /// The node that follows.
    pub child: usize,
}

/// A model to solve.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Model {
    /// Node positions, mm.
    pub nodes: Vec<V3>,
    /// The beams.
    pub beams: Vec<Beam>,
    /// Held degrees of freedom per node.
    pub held: Vec<(usize, Dofs)>,
    /// Rigid links.
    pub links: Vec<RigidLink>,
    /// Loads.
    pub loads: Vec<Load>,
}

/// One element of the mesh: a piece of a beam with one uniform load.
#[derive(Clone, Debug)]
struct Element {
    beam: usize,
    /// Mesh nodes.
    n: [usize; 2],
    /// From / to along the beam.
    s: [Real; 2],
    /// Local axes as rows (x, u, v).
    r: Matrix3<Real>,
    /// Uniform load in local axes, N/mm.
    q: V3,
    /// Released local unknowns (0..12).
    released: Vec<usize>,
}

type M12 = SMatrix<Real, 12, 12>;
type V12 = SMatrix<Real, 12, 1>;

/// The section forces at one station of a beam (internal forces on the
/// face looking towards the beam's end, in the beam's axes `u v x`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Station {
    /// Distance from the beam's start, mm.
    pub s: Real,
    /// Displacement in the model's axes, mm.
    pub displacement: [Real; 3],
    /// Shear along `u`, N.
    pub fu: Real,
    /// Shear along `v`, N.
    pub fv: Real,
    /// Axial force (tension positive), N.
    pub n: Real,
    /// Moment about `u`, N·mm.
    pub mu: Real,
    /// Moment about `v`, N·mm.
    pub mv: Real,
    /// Torque about the beam, N·mm.
    pub t: Real,
}

impl Station {
    /// Axial stress, MPa.
    pub fn s_axial(&self, c: &BeamSection) -> Real {
        self.n / c.area
    }
    /// Bending stress of `mu` at the farthest fibre (unsigned), MPa.
    pub fn s_mu(&self, c: &BeamSection) -> Real {
        self.mu.abs() / c.wu
    }
    /// Bending stress of `mv` at the farthest fibre (unsigned), MPa.
    pub fn s_mv(&self, c: &BeamSection) -> Real {
        self.mv.abs() / c.wv
    }
    /// Largest normal stress of the section, MPa.
    pub fn s_max(&self, c: &BeamSection) -> Real {
        self.s_axial(c) + self.s_mu(c) + self.s_mv(c)
    }
    /// Smallest normal stress of the section, MPa.
    pub fn s_min(&self, c: &BeamSection) -> Real {
        self.s_axial(c) - self.s_mu(c) - self.s_mv(c)
    }
    /// Mean shear stress along `u`, MPa.
    pub fn t_u(&self, c: &BeamSection) -> Real {
        self.fu / c.au
    }
    /// Mean shear stress along `v`, MPa.
    pub fn t_v(&self, c: &BeamSection) -> Real {
        self.fv / c.av
    }
    /// Torsional shear stress at the farthest fibre, MPa.
    pub fn t_t(&self, c: &BeamSection) -> Real {
        self.t.abs() / c.wt
    }
}

/// What a static solve found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Solution {
    /// Displacement and rotation per model node.
    pub nodes: Vec<[Real; 6]>,
    /// Stations per beam, in order along it.
    pub beams: Vec<Vec<Station>>,
    /// Reaction per node of [`Model::held`], same order (force, moment).
    pub reactions: Vec<[Real; 6]>,
}

/// One natural mode.
#[derive(Clone, Debug, PartialEq)]
pub struct Mode {
    /// Frequency, Hz.
    pub hz: Real,
    /// Shape per model node (displacements, rotations; scaled to a largest
    /// displacement of 1).
    pub shape: Vec<[Real; 6]>,
}

/// How unknowns relate to the global matrix: which equation (`usize::MAX` =
/// held) and with which coefficient each node's six values enter.
struct Numbering {
    /// (node, dof) → the unknowns it is a combination of.
    map: Vec<[Vec<(usize, Real)>; 6]>,
    unknowns: usize,
}

const NO_LOAD: V3 = V3::new(0.0, 0.0, 0.0);

/// Tolerance on positions along a beam, mm.
const SNAP: Real = 1e-6;

/// The local axes of a beam from `a` to `b` with the section's `u`, as rows.
fn axes(a: &V3, b: &V3, u: &V3) -> Option<Matrix3<Real>> {
    let x = (b - a).try_normalize(1e-9)?;
    let u = (u - x * x.dot(u)).try_normalize(1e-9)?;
    let v = x.cross(&u);
    Some(Matrix3::from_rows(&[
        x.transpose(),
        u.transpose(),
        v.transpose(),
    ]))
}

/// The local stiffness of an element of length `l` (unknowns: the two
/// ends' `x u v` translations and rotations).
fn stiffness(l: Real, c: &BeamSection, m: &BeamMaterial) -> M12 {
    let mut k = M12::zeros();
    let (e, g) = (m.e, m.g);
    let add = |k: &mut M12, i: usize, j: usize, v: Real| {
        k[(i, j)] += v;
        if i != j {
            k[(j, i)] += v;
        }
    };
    // Axial, torsion.
    let ea = e * c.area / l;
    add(&mut k, 0, 0, ea);
    add(&mut k, 6, 6, ea);
    add(&mut k, 0, 6, -ea);
    let gj = g * c.j / l;
    add(&mut k, 3, 3, gj);
    add(&mut k, 9, 9, gj);
    add(&mut k, 3, 9, -gj);
    // Bending in the x-u plane: rotation about v, stiffness E·Iv.
    let ei = e * c.iv;
    let (a, b, d) = (12.0 * ei / l.powi(3), 6.0 * ei / l.powi(2), 4.0 * ei / l);
    add(&mut k, 1, 1, a);
    add(&mut k, 7, 7, a);
    add(&mut k, 1, 7, -a);
    add(&mut k, 1, 5, b);
    add(&mut k, 1, 11, b);
    add(&mut k, 5, 7, -b);
    add(&mut k, 7, 11, -b);
    add(&mut k, 5, 5, d);
    add(&mut k, 11, 11, d);
    add(&mut k, 5, 11, d / 2.0);
    // Bending in the x-v plane: rotation about u, stiffness E·Iu; a positive
    // rotation about u lowers v along +x, hence the signs.
    let ei = e * c.iu;
    let (a, b, d) = (12.0 * ei / l.powi(3), 6.0 * ei / l.powi(2), 4.0 * ei / l);
    add(&mut k, 2, 2, a);
    add(&mut k, 8, 8, a);
    add(&mut k, 2, 8, -a);
    add(&mut k, 2, 4, -b);
    add(&mut k, 2, 10, -b);
    add(&mut k, 4, 8, b);
    add(&mut k, 8, 10, b);
    add(&mut k, 4, 4, d);
    add(&mut k, 10, 10, d);
    add(&mut k, 4, 10, d / 2.0);
    k
}

/// The consistent mass matrix of an element, local axes (torsional inertia
/// from the polar moment of the section).
fn mass(l: Real, c: &BeamSection, m: &BeamMaterial) -> M12 {
    let mut k = M12::zeros();
    let t = m.rho * c.area * l;
    let add = |k: &mut M12, i: usize, j: usize, v: Real| {
        k[(i, j)] += v;
        if i != j {
            k[(j, i)] += v;
        }
    };
    add(&mut k, 0, 0, t / 3.0);
    add(&mut k, 6, 6, t / 3.0);
    add(&mut k, 0, 6, t / 6.0);
    let ip = m.rho * (c.iu + c.iv) * l;
    add(&mut k, 3, 3, ip / 3.0);
    add(&mut k, 9, 9, ip / 3.0);
    add(&mut k, 3, 9, ip / 6.0);
    let f = t / 420.0;
    // u: rotation about v; v: rotation about u with the opposite coupling.
    for (w, r, s) in [(1, 5, 1.0), (2, 4, -1.0)] {
        let (w2, r2) = (w + 6, r + 6);
        add(&mut k, w, w, 156.0 * f);
        add(&mut k, w2, w2, 156.0 * f);
        add(&mut k, w, w2, 54.0 * f);
        add(&mut k, w, r, s * 22.0 * l * f);
        add(&mut k, w, r2, -s * 13.0 * l * f);
        add(&mut k, w2, r, s * 13.0 * l * f);
        add(&mut k, w2, r2, -s * 22.0 * l * f);
        add(&mut k, r, r, 4.0 * l * l * f);
        add(&mut k, r2, r2, 4.0 * l * l * f);
        add(&mut k, r, r2, -3.0 * l * l * f);
    }
    k
}

/// The forces a uniform load `q` (local) puts on the two ends of a clamped
/// element, as loads on the unknowns.
fn fixed_end(l: Real, q: &V3) -> V12 {
    let mut f = V12::zeros();
    f[0] = q.x * l / 2.0;
    f[6] = q.x * l / 2.0;
    // Along u: the end moments about v.
    f[1] = q.y * l / 2.0;
    f[7] = q.y * l / 2.0;
    f[5] = q.y * l * l / 12.0;
    f[11] = -q.y * l * l / 12.0;
    // Along v: the end moments about u, opposite sign.
    f[2] = q.z * l / 2.0;
    f[8] = q.z * l / 2.0;
    f[4] = -q.z * l * l / 12.0;
    f[10] = q.z * l * l / 12.0;
    f
}

/// The 12×12 rotation from global to local unknowns.
fn rotation(r: &Matrix3<Real>) -> M12 {
    let mut t = M12::zeros();
    for b in 0..4 {
        t.fixed_view_mut::<3, 3>(b * 3, b * 3).copy_from(r);
    }
    t
}

/// A symmetric matrix in skyline storage (the upper triangle by columns),
/// factored as L·D·Lᵀ in place.
struct Skyline {
    /// First stored row of each column.
    first: Vec<usize>,
    /// Where each column's values start in `v`.
    at: Vec<usize>,
    v: Vec<Real>,
}

impl Skyline {
    fn new(first: Vec<usize>) -> Self {
        let mut at = Vec::with_capacity(first.len() + 1);
        let mut n = 0;
        at.push(0);
        for (j, f) in first.iter().enumerate() {
            n += j + 1 - f;
            at.push(n);
        }
        Self {
            first,
            at,
            v: vec![0.0; n],
        }
    }

    fn slot(&self, i: usize, j: usize) -> usize {
        self.at[j] + (i - self.first[j])
    }

    fn add(&mut self, i: usize, j: usize, x: Real) {
        let (i, j) = if i <= j { (i, j) } else { (j, i) };
        let s = self.slot(i, j);
        self.v[s] += x;
    }

    /// L·D·Lᵀ in place. `None` when a pivot vanishes: a mechanism.
    fn factor(&mut self) -> Option<()> {
        let n = self.first.len();
        let largest = (0..n)
            .map(|j| self.v[self.slot(j, j)].abs())
            .fold(0.0, Real::max);
        for j in 0..n {
            let fj = self.first[j];
            let aj = self.at[j];
            // Columns of the upper triangle: reduce rows top-down.
            for i in fj..j {
                let fi = self.first[i];
                let from = fi.max(fj);
                let ai = self.at[i];
                let mut sum = 0.0;
                for r in from..i {
                    sum += self.v[ai + r - fi] * self.v[aj + r - fj];
                }
                self.v[aj + i - fj] -= sum;
            }
            let mut d = self.v[aj + j - fj];
            for i in fj..j {
                let di = self.v[self.at[i] + i - self.first[i]];
                let u = self.v[aj + i - fj];
                let l = u / di;
                d -= u * l;
                self.v[aj + i - fj] = l;
            }
            if !d.is_finite() || d.abs() <= 1e-11 * largest.max(1e-300) {
                return None;
            }
            self.v[aj + j - fj] = d;
        }
        Some(())
    }

    /// Solves with the factors.
    fn solve(&self, b: &mut [Real]) {
        let n = self.first.len();
        for j in 0..n {
            let fj = self.first[j];
            let aj = self.at[j];
            let sum: Real = (fj..j).map(|r| self.v[aj + r - fj] * b[r]).sum();
            b[j] -= sum;
        }
        for (j, x) in b.iter_mut().enumerate().take(n) {
            *x /= self.v[self.at[j] + j - self.first[j]];
        }
        for j in (0..n).rev() {
            let fj = self.first[j];
            let aj = self.at[j];
            for r in fj..j {
                b[r] -= self.v[aj + r - fj] * b[j];
            }
        }
    }
}

/// A beam cut at its stations.
struct Mesh {
    /// Mesh node positions (the model's first).
    nodes: Vec<V3>,
    elements: Vec<Element>,
    /// Per beam, element indices in order.
    per_beam: Vec<Vec<usize>>,
    /// Node loads (mesh node, force, moment).
    node_loads: Vec<(usize, V3, V3)>,
}

impl Model {
    fn check(&self) -> Result<(), String> {
        if self.beams.is_empty() {
            return Err("the model has no beams".into());
        }
        let n = self.nodes.len();
        for b in &self.beams {
            if b.start >= n || b.end >= n || b.start == b.end {
                return Err("a beam has no valid nodes".into());
            }
            let c = &b.section;
            let m = &b.material;
            if [c.area, c.iu, c.iv, c.j, m.e, m.g]
                .iter()
                .any(|v| !(v.is_finite() && *v > 0.0))
            {
                return Err("a beam has no section or material numbers".into());
            }
        }
        for l in &self.links {
            if l.parent >= n || l.child >= n || l.parent == l.child {
                return Err("a rigid link has no valid nodes".into());
            }
        }
        Ok(())
    }

    /// Cuts every beam at its loads' stations and, when `at_least` is more
    /// than one, into that many equal pieces as well.
    fn mesh(&self, at_least: usize) -> Result<Mesh, String> {
        let mut nodes = self.nodes.clone();
        let mut elements = Vec::new();
        let mut per_beam = Vec::with_capacity(self.beams.len());
        let mut node_loads = Vec::new();
        for l in &self.loads {
            if let Load::Node {
                node,
                force,
                moment,
            } = l
            {
                if *node >= self.nodes.len() {
                    return Err("a load has no valid node".into());
                }
                node_loads.push((*node, *force, *moment));
            }
        }
        for (ib, b) in self.beams.iter().enumerate() {
            let (pa, pb) = (self.nodes[b.start], self.nodes[b.end]);
            let len = (pb - pa).norm();
            let r = axes(&pa, &pb, &b.u).ok_or("a beam has no length or no section axis")?;
            let mut cuts: BTreeSet<i64> = BTreeSet::new();
            let key = |s: Real| (s / SNAP).round() as i64;
            cuts.insert(key(0.0));
            cuts.insert(key(len));
            for k in 1..at_least {
                cuts.insert(key(len * k as Real / at_least as Real));
            }
            let inside = |s: Real| s.clamp(0.0, len);
            for l in &self.loads {
                match l {
                    Load::Point { beam, at, .. } if *beam == ib => {
                        cuts.insert(key(inside(*at)));
                    }
                    Load::Uniform { beam, from, to, .. } if *beam == ib => {
                        cuts.insert(key(inside(*from)));
                        cuts.insert(key(inside(*to)));
                    }
                    _ => {}
                }
            }
            let cuts: Vec<Real> = cuts.into_iter().map(|k| k as Real * SNAP).collect();
            let mut ids = Vec::with_capacity(cuts.len());
            for (i, s) in cuts.iter().enumerate() {
                if i == 0 {
                    ids.push(b.start);
                } else if i + 1 == cuts.len() {
                    ids.push(b.end);
                } else {
                    nodes.push(pa + (pb - pa) * (s / len));
                    ids.push(nodes.len() - 1);
                }
            }
            for l in &self.loads {
                if let Load::Point {
                    beam,
                    at,
                    force,
                    moment,
                } = l
                    && *beam == ib
                {
                    let k = cuts
                        .iter()
                        .position(|s| (s - inside(*at)).abs() <= 2.0 * SNAP)
                        .ok_or("a load's station was lost")?;
                    node_loads.push((ids[k], *force, *moment));
                }
            }
            let mut own = Vec::with_capacity(cuts.len());
            for k in 0..cuts.len() - 1 {
                let (s0, s1) = (cuts[k], cuts[k + 1]);
                let mid = (s0 + s1) / 2.0;
                let mut q = NO_LOAD;
                for l in &self.loads {
                    match l {
                        Load::Uniform {
                            beam,
                            from,
                            to,
                            q: w,
                        } if *beam == ib && mid > from.min(*to) && mid < from.max(*to) => {
                            q += w;
                        }
                        Load::Gravity { g } => q += g * (b.material.rho * b.section.area),
                        _ => {}
                    }
                }
                let mut released = Vec::new();
                if k == 0 {
                    released.extend((0..6).filter(|&d| b.release_start[d]));
                }
                if k + 2 == cuts.len() {
                    released.extend((0..6).filter(|&d| b.release_end[d]).map(|d| d + 6));
                }
                own.push(elements.len());
                elements.push(Element {
                    beam: ib,
                    n: [ids[k], ids[k + 1]],
                    s: [s0, s1],
                    r,
                    q: r * q,
                    released,
                });
            }
            per_beam.push(own);
        }
        Ok(Mesh {
            nodes,
            elements,
            per_beam,
            node_loads,
        })
    }

    /// Unknown numbers: held values vanish, a rigid link's child is a
    /// combination of its parent's.
    fn numbering(&self, nodes: &[V3]) -> Result<Numbering, String> {
        let n = nodes.len();
        let mut parent: Vec<Option<usize>> = vec![None; n];
        for l in &self.links {
            if parent[l.child].is_some() {
                return Err("a node is the child of two rigid links".into());
            }
            parent[l.child] = Some(l.parent);
        }
        let root = |mut i: usize| -> Result<usize, String> {
            let mut steps = 0;
            while let Some(p) = parent[i] {
                i = p;
                steps += 1;
                if steps > n {
                    return Err("rigid links form a ring".into());
                }
            }
            Ok(i)
        };
        // A child's held values hold its root.
        let mut held = vec![[false; 6]; n];
        for (node, d) in &self.held {
            if *node >= n {
                return Err("a constraint has no valid node".into());
            }
            let r = root(*node)?;
            for k in 0..6 {
                held[r][k] |= d[k];
            }
        }
        let mut number = vec![[usize::MAX; 6]; n];
        let mut unknowns = 0;
        for i in 0..n {
            if parent[i].is_none() {
                for k in 0..6 {
                    if !held[i][k] {
                        number[i][k] = unknowns;
                        unknowns += 1;
                    }
                }
            }
        }
        let mut map: Vec<[Vec<(usize, Real)>; 6]> = Vec::with_capacity(n);
        for i in 0..n {
            let r = root(i)?;
            let mut m: [Vec<(usize, Real)>; 6] = Default::default();
            let term = |d: usize, c: Real, v: &mut Vec<(usize, Real)>| {
                if number[r][d] != usize::MAX && c != 0.0 {
                    v.push((number[r][d], c));
                }
            };
            for k in 0..3 {
                term(k, 1.0, &mut m[k]);
                term(k + 3, 1.0, &mut m[k + 3]);
            }
            if r != i {
                // u_child = u_root + θ × (p_child − p_root).
                let d = nodes[i] - nodes[r];
                term(4, d.z, &mut m[0]);
                term(5, -d.y, &mut m[0]);
                term(5, d.x, &mut m[1]);
                term(3, -d.z, &mut m[1]);
                term(3, d.y, &mut m[2]);
                term(4, -d.x, &mut m[2]);
            }
            map.push(m);
        }
        Ok(Numbering { map, unknowns })
    }

    /// An element's matrix after static condensation of its released
    /// unknowns, with its load vector, in global axes.
    fn element_global(&self, e: &Element, l: Real) -> Result<(M12, V12), String> {
        let b = &self.beams[e.beam];
        let k = stiffness(l, &b.section, &b.material);
        let f = fixed_end(l, &e.q);
        let (k, f) = condense(&k, &f, &e.released)?;
        let t = rotation(&e.r);
        Ok((t.transpose() * k * t, t.transpose() * f))
    }

    /// Solves the static problem; `points` result stations per beam (at
    /// least 2, the ends included).
    pub fn solve_static(&self, points: usize) -> Result<Solution, String> {
        self.check()?;
        let mesh = self.mesh(1)?;
        let num = self.numbering(&mesh.nodes)?;
        if num.unknowns == 0 {
            return Err("every degree of freedom is held: nothing can move".into());
        }
        let order = rcm_order(&mesh, &num);
        let mut kk = skyline_for(&mesh, &num, &order);
        let mut f = vec![0.0; num.unknowns];
        for e in &mesh.elements {
            let l = e.s[1] - e.s[0];
            let (k, fe) = self.element_global(e, l)?;
            assemble(&mut kk, Some(&mut f), &k, &fe, e, &num, &order);
        }
        for (n, force, moment) in &mesh.node_loads {
            for d in 0..6 {
                let v = if d < 3 { force[d] } else { moment[d - 3] };
                for (u, c) in &num.map[*n][d] {
                    f[order[*u]] += c * v;
                }
            }
        }
        kk.factor()
            .ok_or("the frame can move freely: add constraints")?;
        kk.solve(&mut f);
        let value = |n: usize, d: usize| -> Real {
            num.map[n][d].iter().map(|(u, c)| c * f[order[*u]]).sum()
        };
        let node_u: Vec<[Real; 6]> = (0..mesh.nodes.len())
            .map(|n| std::array::from_fn(|d| value(n, d)))
            .collect();
        // Element end forces (on the element, local) and its 12 local
        // unknowns with the released ones recovered.
        let ends: Vec<(V12, V12)> = mesh
            .elements
            .iter()
            .map(|e| self.element_result(e, &node_u))
            .collect::<Result<_, _>>()?;
        let mut beams = Vec::with_capacity(self.beams.len());
        for (ib, own) in mesh.per_beam.iter().enumerate() {
            let len = (self.nodes[self.beams[ib].end] - self.nodes[self.beams[ib].start]).norm();
            let mut stations: Vec<Station> = Vec::new();
            let n = points.max(2);
            let mut at: Vec<Real> = (0..n).map(|k| len * k as Real / (n - 1) as Real).collect();
            for &i in own {
                at.push(mesh.elements[i].s[0]);
                at.push(mesh.elements[i].s[1]);
            }
            at.sort_by(Real::total_cmp);
            at.dedup_by(|a, b| (*a - *b).abs() <= 2.0 * SNAP);
            for s in at {
                // Elements on both sides of a load's station report their
                // own value: take the one that starts there, the last at the
                // end.
                let i = own
                    .iter()
                    .copied()
                    .find(|&i| s < mesh.elements[i].s[1] - SNAP)
                    .or(own.last().copied())
                    .ok_or("a beam has no elements")?;
                stations.push(self.station(&mesh.elements[i], &ends[i], s));
            }
            beams.push(stations);
        }
        // Reactions: what the elements and the loads leave at held nodes.
        let mut rest = vec![[0.0; 6]; mesh.nodes.len()];
        for (e, (_, fe)) in mesh.elements.iter().zip(&ends) {
            let t = rotation(&e.r).transpose() * fe;
            for d in 0..6 {
                rest[e.n[0]][d] += t[d];
                rest[e.n[1]][d] += t[d + 6];
            }
        }
        for (n, force, moment) in &mesh.node_loads {
            for d in 0..3 {
                rest[*n][d] -= force[d];
                rest[*n][d + 3] -= moment[d];
            }
        }
        let reactions = self
            .held
            .iter()
            .map(|(n, d)| std::array::from_fn(|k| if d[k] { rest[*n][k] } else { 0.0 }))
            .collect();
        Ok(Solution {
            nodes: node_u[..self.nodes.len()].to_vec(),
            beams,
            reactions,
        })
    }

    /// An element's local unknowns (released ones recovered) and the forces
    /// at its ends, on the element, local axes, loads taken off.
    fn element_result(&self, e: &Element, u: &[[Real; 6]]) -> Result<(V12, V12), String> {
        let b = &self.beams[e.beam];
        let l = e.s[1] - e.s[0];
        let k = stiffness(l, &b.section, &b.material);
        let f = fixed_end(l, &e.q);
        let t = rotation(&e.r);
        let mut g = V12::zeros();
        for d in 0..6 {
            g[d] = u[e.n[0]][d];
            g[d + 6] = u[e.n[1]][d];
        }
        let mut local = t * g;
        recover_released(&k, &f, &e.released, &mut local)?;
        let end = k * local - f;
        Ok((local, end))
    }

    /// Displacement and section forces at `s` on element `e`.
    fn station(&self, e: &Element, (u, end): &(V12, V12), s: Real) -> Station {
        let b = &self.beams[e.beam];
        let l = e.s[1] - e.s[0];
        let x = (s - e.s[0]).clamp(0.0, l);
        let xi = x / l;
        // Shape functions of the cubic.
        let n1 = 1.0 - 3.0 * xi * xi + 2.0 * xi.powi(3);
        let n2 = x * (1.0 - xi).powi(2);
        let n3 = 3.0 * xi * xi - 2.0 * xi.powi(3);
        let n4 = x * xi * (xi - 1.0);
        let ux = u[0] * (1.0 - xi) + u[6] * xi;
        let mut w_u = n1 * u[1] + n2 * u[5] + n3 * u[7] + n4 * u[11];
        let mut w_v = n1 * u[2] - n2 * u[4] + n3 * u[8] - n4 * u[10];
        // The uniform load's own curve between clamped ends.
        let shape = x * x * (l - x) * (l - x) / 24.0;
        let (ei_v, ei_u) = (b.material.e * b.section.iv, b.material.e * b.section.iu);
        w_u += e.q.y * shape / ei_v;
        w_v += e.q.z * shape / ei_u;
        let ea_shape = x * (l - x) / 2.0 / (b.material.e * b.section.area);
        let ux = ux + e.q.x * ea_shape;
        let disp = e.r.transpose() * V3::new(ux, w_u, w_v);
        // Statics of the piece from the start to the cut.
        let f1 = V3::new(end[0], end[1], end[2]);
        let m1 = V3::new(end[3], end[4], end[5]);
        let qx = e.q * x;
        let force = -(f1 + qx);
        let arm = V3::new(-x, 0.0, 0.0);
        let moment = -(m1 + arm.cross(&f1) + (arm / 2.0).cross(&qx));
        Station {
            s,
            displacement: [disp.x, disp.y, disp.z],
            fu: force.y,
            fv: force.z,
            n: force.x,
            mu: moment.y,
            mv: moment.z,
            t: moment.x,
        }
    }

    /// The lowest `count` natural modes (consistent masses; each beam in
    /// at least `pieces` elements). A free model has its six rigid modes at
    /// 0 Hz first.
    pub fn solve_modes(&self, count: usize, pieces: usize) -> Result<Vec<Mode>, String> {
        self.check()?;
        let mesh = self.mesh(pieces.max(2))?;
        let num = self.numbering(&mesh.nodes)?;
        if num.unknowns == 0 {
            return Err("every degree of freedom is held: nothing can vibrate".into());
        }
        let n = num.unknowns;
        let order = rcm_order(&mesh, &num);
        let mut kk = skyline_for(&mesh, &num, &order);
        let mut mm = skyline_for(&mesh, &num, &order);
        for e in &mesh.elements {
            let l = e.s[1] - e.s[0];
            let b = &self.beams[e.beam];
            let (k, f) = self.element_global(e, l)?;
            assemble(&mut kk, None, &k, &f, e, &num, &order);
            let t = rotation(&e.r);
            let me = t.transpose() * mass(l, &b.section, &b.material) * t;
            assemble(&mut mm, None, &me, &V12::zeros(), e, &num, &order);
        }
        let mass_values = mm.v.clone();
        let m_times = |x: &DVector<Real>| -> DVector<Real> {
            let mut y = DVector::zeros(n);
            for j in 0..n {
                let (f, a) = (mm.first[j], mm.at[j]);
                for i in f..=j {
                    let v = mass_values[a + i - f];
                    y[i] += v * x[j];
                    if i != j {
                        y[j] += v * x[i];
                    }
                }
            }
            y
        };
        // A small shift lets a free model factor: K + σM.
        let trace_k: Real = (0..n).map(|j| kk.v[kk.slot(j, j)].abs()).sum::<Real>() / n as Real;
        let trace_m: Real = (0..n).map(|j| mm.v[mm.slot(j, j)].abs()).sum::<Real>() / n as Real;
        let sigma = 1e-6 * trace_k / trace_m.max(1e-300);
        for (i, v) in kk.v.iter_mut().enumerate() {
            *v += sigma * mass_values[i];
        }
        kk.factor().ok_or("the frame cannot be factored")?;
        // Subspace iteration.
        let want = count.clamp(1, n);
        let size = (want * 2).max(want + 8).min(n);
        let mut x = DMatrix::<Real>::zeros(n, size);
        for j in 0..size {
            for i in 0..n {
                let k = (i * 7919 + j * 104729) % 1009;
                x[(i, j)] = k as Real / 1009.0 - 0.5 + if i % size == j { 1.0 } else { 0.0 };
            }
        }
        let mut last = vec![0.0; want];
        let mut lambda = vec![0.0; size];
        let mut vectors = x.clone();
        for iter in 0..200 {
            let mut y = DMatrix::<Real>::zeros(n, size);
            // M·x is the right-hand side and the reduced stiffness' factor:
            // once per column (per pair it was most of the run's time).
            let mut mx: Vec<DVector<Real>> = Vec::with_capacity(size);
            for j in 0..size {
                let product = m_times(&x.column(j).into_owned());
                let mut b = product.clone();
                kk.solve(b.as_mut_slice());
                y.set_column(j, &b);
                mx.push(product);
            }
            // Rayleigh-Ritz on span(y): K-reduced via y'·M·x and y'·M·y.
            let my: Vec<DVector<Real>> = (0..size)
                .map(|j| m_times(&y.column(j).into_owned()))
                .collect();
            let (mut a, mut b) = (DMatrix::zeros(size, size), DMatrix::zeros(size, size));
            for i in 0..size {
                for j in 0..size {
                    a[(i, j)] = y.column(i).dot(&mx[j]);
                    b[(i, j)] = y.column(i).dot(&my[j]);
                }
            }
            let a = (&a + a.transpose()) * 0.5;
            let b = (&b + b.transpose()) * 0.5;
            let chol = b.clone().cholesky().ok_or("the modes lost their basis")?;
            let l_inv = chol.l().try_inverse().ok_or("the modes lost their basis")?;
            let c = &l_inv * &a * l_inv.transpose();
            let eig = nalgebra::SymmetricEigen::new((&c + c.transpose()) * 0.5);
            let mut idx: Vec<usize> = (0..size).collect();
            idx.sort_by(|&p, &q| eig.eigenvalues[p].total_cmp(&eig.eigenvalues[q]));
            let basis = l_inv.transpose() * &eig.eigenvectors;
            let mut nx = DMatrix::<Real>::zeros(n, size);
            for (k, &i) in idx.iter().enumerate() {
                let col = &y * basis.column(i);
                let norm = col.dot(&m_times(&col)).abs().sqrt().max(1e-300);
                nx.set_column(k, &(col / norm));
                lambda[k] = eig.eigenvalues[i];
            }
            x = nx.clone();
            vectors = nx;
            let close =
                (0..want).all(|k| (lambda[k] - last[k]).abs() <= 1e-8 * lambda[k].abs().max(1e-12));
            last[..want].copy_from_slice(&lambda[..want]);
            if close && iter > 2 {
                break;
            }
        }
        let modes = (0..want)
            .map(|k| {
                let hz = ((lambda[k] - sigma).max(0.0)).sqrt() / (2.0 * std::f64::consts::PI);
                let v = vectors.column(k);
                let raw: Vec<[Real; 6]> = (0..self.nodes.len())
                    .map(|i| {
                        std::array::from_fn(|d| {
                            num.map[i][d].iter().map(|(u, c)| c * v[order[*u]]).sum()
                        })
                    })
                    .collect();
                let big = raw
                    .iter()
                    .map(|u| (u[0] * u[0] + u[1] * u[1] + u[2] * u[2]).sqrt())
                    .fold(0.0, Real::max)
                    .max(1e-300);
                Mode {
                    hz,
                    shape: raw
                        .iter()
                        .map(|u| std::array::from_fn(|d| u[d] / big))
                        .collect(),
                }
            })
            .collect();
        Ok(modes)
    }
}

/// Removes the released unknowns from an element's matrix and load.
fn condense(k: &M12, f: &V12, released: &[usize]) -> Result<(M12, V12), String> {
    if released.is_empty() {
        return Ok((*k, *f));
    }
    let m = released.len();
    let keep: Vec<usize> = (0..12).filter(|i| !released.contains(i)).collect();
    let mut krr = DMatrix::<Real>::zeros(m, m);
    for (a, &i) in released.iter().enumerate() {
        for (b, &j) in released.iter().enumerate() {
            krr[(a, b)] = k[(i, j)];
        }
    }
    let inv = krr
        .try_inverse()
        .ok_or("a beam is released at both ends about the same axis")?;
    let (mut ko, mut fo) = (M12::zeros(), V12::zeros());
    for &i in &keep {
        let mut fi = f[i];
        for (a, &ra) in released.iter().enumerate() {
            for (b, &rb) in released.iter().enumerate() {
                fi -= k[(i, ra)] * inv[(a, b)] * f[rb];
            }
        }
        fo[i] = fi;
        for &j in &keep {
            let mut v = k[(i, j)];
            for (a, &ra) in released.iter().enumerate() {
                for (b, &rb) in released.iter().enumerate() {
                    v -= k[(i, ra)] * inv[(a, b)] * k[(rb, j)];
                }
            }
            ko[(i, j)] = v;
        }
    }
    Ok((ko, fo))
}

/// Fills in the released unknowns of `u` from the kept ones.
fn recover_released(k: &M12, f: &V12, released: &[usize], u: &mut V12) -> Result<(), String> {
    if released.is_empty() {
        return Ok(());
    }
    let m = released.len();
    let keep: Vec<usize> = (0..12).filter(|i| !released.contains(i)).collect();
    let mut krr = DMatrix::<Real>::zeros(m, m);
    let mut rhs = DVector::<Real>::zeros(m);
    for (a, &i) in released.iter().enumerate() {
        rhs[a] = f[i] - keep.iter().map(|&j| k[(i, j)] * u[j]).sum::<Real>();
        for (b, &j) in released.iter().enumerate() {
            krr[(a, b)] = k[(i, j)];
        }
    }
    let x = krr
        .lu()
        .solve(&rhs)
        .ok_or("a beam is released at both ends about the same axis")?;
    for (a, &i) in released.iter().enumerate() {
        u[i] = x[a];
    }
    Ok(())
}

/// Each element's unknowns.
fn element_unknowns(e: &Element, num: &Numbering) -> Vec<usize> {
    let mut v: Vec<usize> = (0..2)
        .flat_map(|side| (0..6).map(move |d| (side, d)))
        .flat_map(|(side, d)| num.map[e.n[side]][d].iter().map(|t| t.0))
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// Reverse Cuthill-McKee numbering of the unknowns: new index of each.
fn rcm_order(mesh: &Mesh, num: &Numbering) -> Vec<usize> {
    let n = num.unknowns;
    let mut near: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); n];
    for e in &mesh.elements {
        let u = element_unknowns(e, num);
        for &a in &u {
            near[a].extend(u.iter().copied().filter(|b| *b != a));
        }
    }
    let mut done = vec![false; n];
    let mut seq = Vec::with_capacity(n);
    let mut starts: Vec<usize> = (0..n).collect();
    starts.sort_by_key(|&i| near[i].len());
    for s in starts {
        if done[s] {
            continue;
        }
        done[s] = true;
        let mut at = seq.len();
        seq.push(s);
        while at < seq.len() {
            let v = seq[at];
            at += 1;
            let mut next: Vec<usize> = near[v].iter().copied().filter(|&u| !done[u]).collect();
            next.sort_by_key(|&u| near[u].len());
            for u in next {
                done[u] = true;
                seq.push(u);
            }
        }
    }
    let mut order = vec![0; n];
    for (i, &v) in seq.iter().rev().enumerate() {
        order[v] = i;
    }
    order
}

/// An empty matrix with the skyline these elements fill.
fn skyline_for(mesh: &Mesh, num: &Numbering, order: &[usize]) -> Skyline {
    let n = num.unknowns;
    let mut first: Vec<usize> = (0..n).collect();
    for e in &mesh.elements {
        let u: Vec<usize> = element_unknowns(e, num).iter().map(|&x| order[x]).collect();
        if let Some(&low) = u.iter().min() {
            for &x in &u {
                first[x] = first[x].min(low);
            }
        }
    }
    Skyline::new(first)
}

/// Adds an element's matrix (and load) to the global ones.
fn assemble(
    k: &mut Skyline,
    mut f: Option<&mut Vec<Real>>,
    ke: &M12,
    fe: &V12,
    e: &Element,
    num: &Numbering,
    order: &[usize],
) {
    let at = |i: usize| -> &Vec<(usize, Real)> {
        if i < 6 {
            &num.map[e.n[0]][i]
        } else {
            &num.map[e.n[1]][i - 6]
        }
    };
    for i in 0..12 {
        if let Some(f) = f.as_deref_mut() {
            for (u, c) in at(i) {
                f[order[*u]] += c * fe[i];
            }
        }
        for j in 0..12 {
            let v = ke[(i, j)];
            if v == 0.0 {
                continue;
            }
            for (ui, ci) in at(i) {
                for (uj, cj) in at(j) {
                    let (a, b) = (order[*ui], order[*uj]);
                    if a <= b {
                        k.add(a, b, ci * cj * v);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const E: Real = 210_000.0;
    const G: Real = 81_000.0;

    fn steel() -> BeamMaterial {
        BeamMaterial {
            e: E,
            g: G,
            rho: 7.85e-9,
        }
    }

    /// 100 × 50 solid-ish numbers.
    fn section() -> BeamSection {
        BeamSection {
            area: 1000.0,
            iu: 2.0e6,
            iv: 5.0e5,
            j: 1.0e6,
            wu: 4.0e4,
            wv: 2.0e4,
            wt: 3.0e4,
            au: 500.0,
            av: 500.0,
        }
    }

    fn beam(a: usize, b: usize, u: V3) -> Beam {
        Beam {
            start: a,
            end: b,
            u,
            section: section(),
            material: steel(),
            release_start: [false; 6],
            release_end: [false; 6],
        }
    }

    const FIX: Dofs = [true; 6];
    const PIN: Dofs = [true, true, true, false, false, false];

    fn along_x(l: Real) -> Model {
        Model {
            nodes: vec![V3::zeros(), V3::new(l, 0.0, 0.0)],
            beams: vec![beam(0, 1, V3::y())],
            ..Default::default()
        }
    }

    fn close(got: Real, want: Real, rel: Real, what: &str) {
        assert!(
            (got - want).abs() <= rel * want.abs().max(1e-9),
            "{what}: got {got}, want {want}"
        );
    }

    /// Beam along +X with u = +Y has v = X × Y = +Z: a tip load along Z
    /// bends about u (Iu), one along Y about v (Iv).
    #[test]
    fn cantilever_tip_load_about_both_axes() {
        let l = 1000.0;
        let mut m = along_x(l);
        m.held.push((0, FIX));
        m.loads.push(Load::Node {
            node: 1,
            force: V3::new(0.0, 0.0, -500.0),
            moment: NO_LOAD,
        });
        let s = m.solve_static(11).unwrap();
        close(
            s.nodes[1][2],
            -500.0 * l.powi(3) / (3.0 * E * 2.0e6),
            1e-9,
            "tip z",
        );
        let root = s.beams[0][0];
        close(root.mu.abs(), 500.0 * l, 1e-9, "root moment");
        close(root.fv.abs(), 500.0, 1e-9, "shear");
        // Midpoint moment is half the root's; the same sign.
        let mid = s.beams[0][5];
        close(mid.mu, root.mu / 2.0, 1e-9, "mid moment");
        // Reaction balances.
        close(s.reactions[0][2], 500.0, 1e-9, "reaction");
        close(s.reactions[0][4].abs(), 500.0 * l, 1e-9, "reaction moment");

        let mut m = along_x(l);
        m.held.push((0, FIX));
        m.loads.push(Load::Node {
            node: 1,
            force: V3::new(0.0, 300.0, 0.0),
            moment: NO_LOAD,
        });
        let s = m.solve_static(2).unwrap();
        close(
            s.nodes[1][1],
            300.0 * l.powi(3) / (3.0 * E * 5.0e5),
            1e-9,
            "tip y",
        );
    }

    /// Simply supported under a uniform load: 5qL⁴/384EI at the middle,
    /// qL²/8 there, qL/2 at each support, curve exact between nodes.
    #[test]
    fn simply_supported_uniform_load() {
        let l = 2000.0;
        let q = -3.0;
        let mut m = along_x(l);
        m.held.push((0, [true, true, true, true, false, false]));
        m.held.push((1, [false, true, true, false, false, false]));
        m.loads.push(Load::Uniform {
            beam: 0,
            from: 0.0,
            to: l,
            q: V3::new(0.0, 0.0, q),
        });
        let s = m.solve_static(21).unwrap();
        let mid = s.beams[0][10];
        close(mid.s, l / 2.0, 1e-9, "station");
        close(
            mid.displacement[2],
            5.0 * q * l.powi(4) / (384.0 * E * 2.0e6),
            1e-9,
            "mid deflection",
        );
        close(mid.mu.abs(), q.abs() * l * l / 8.0, 1e-9, "mid moment");
        close(s.reactions[0][2], -q * l / 2.0, 1e-9, "support");
    }

    /// A point load inside the span: the deflection under it (Pa²b²/3EIL)
    /// and the moment Pab/L.
    #[test]
    fn point_load_inside_a_span() {
        let (l, a) = (1500.0, 500.0);
        let b = l - a;
        let p = 1000.0;
        let mut m = along_x(l);
        m.held.push((0, [true, true, true, true, false, false]));
        m.held.push((1, [false, true, true, false, false, false]));
        m.loads.push(Load::Point {
            beam: 0,
            at: a,
            force: V3::new(0.0, 0.0, -p),
            moment: NO_LOAD,
        });
        let s = m.solve_static(16).unwrap();
        let st = s.beams[0].iter().find(|x| (x.s - a).abs() < 1e-3).unwrap();
        close(
            st.displacement[2],
            -p * a * a * b * b / (3.0 * E * 2.0e6 * l),
            1e-9,
            "under load",
        );
        close(st.mu.abs(), p * a * b / l, 1e-9, "moment");
    }

    /// Axial load and torsion of a bar.
    #[test]
    fn axial_and_torsion() {
        let l = 800.0;
        let mut m = along_x(l);
        m.held.push((0, FIX));
        m.loads.push(Load::Node {
            node: 1,
            force: V3::new(2000.0, 0.0, 0.0),
            moment: V3::new(5000.0, 0.0, 0.0),
        });
        let s = m.solve_static(3).unwrap();
        close(s.nodes[1][0], 2000.0 * l / (E * 1000.0), 1e-9, "elongation");
        close(s.nodes[1][3], 5000.0 * l / (G * 1.0e6), 1e-9, "twist");
        let c = section();
        let st = s.beams[0][1];
        close(st.n, 2000.0, 1e-9, "axial");
        close(st.s_axial(&c), 2.0, 1e-9, "stress");
        close(st.t.abs(), 5000.0, 1e-9, "torque");
        close(st.t_t(&c), 5000.0 / 3.0e4, 1e-9, "torsional stress");
    }

    /// A hinge (Release of both bending rotations at the joint) turns a
    /// clamped-clamped beam loaded at the joint into two cantilevers.
    #[test]
    fn a_released_end_is_a_hinge() {
        let l = 1000.0;
        let mut b0 = beam(0, 1, V3::y());
        b0.release_end = [false, false, false, false, true, true];
        let m = Model {
            nodes: vec![
                V3::zeros(),
                V3::new(l, 0.0, 0.0),
                V3::new(2.0 * l, 0.0, 0.0),
            ],
            beams: vec![b0, beam(1, 2, V3::y())],
            held: vec![(0, FIX), (2, FIX)],
            loads: vec![Load::Node {
                node: 1,
                force: V3::new(0.0, 0.0, -600.0),
                moment: NO_LOAD,
            }],
            ..Default::default()
        };
        let s = m.solve_static(5).unwrap();
        // The hinge carries no moment.
        let last = s.beams[0].last().unwrap();
        assert!(last.mu.abs() < 1e-6, "{}", last.mu);
        // Two cantilevers of equal stiffness share the load.
        close(
            s.nodes[1][2],
            -300.0 * l.powi(3) / (3.0 * E * 2.0e6),
            1e-9,
            "joint",
        );
    }

    /// Without enough constraints the solve refuses instead of inventing
    /// a result.
    #[test]
    fn a_mechanism_is_refused() {
        let mut m = along_x(1000.0);
        m.held.push((0, PIN));
        m.loads.push(Load::Node {
            node: 1,
            force: V3::new(0.0, 0.0, -1.0),
            moment: NO_LOAD,
        });
        assert!(m.solve_static(2).unwrap_err().contains("freely"));
    }

    /// A portal frame: legs fixed at the base, equilibrium of the reactions
    /// against the load, and symmetry under a load at the middle.
    #[test]
    fn portal_frame_balances_and_is_symmetric() {
        let (w, h) = (2000.0, 1000.0);
        let m = Model {
            nodes: vec![
                V3::zeros(),
                V3::new(0.0, 0.0, h),
                V3::new(w, 0.0, h),
                V3::new(w, 0.0, 0.0),
            ],
            beams: vec![
                beam(0, 1, V3::y()),
                beam(1, 2, V3::y()),
                beam(2, 3, V3::y()),
            ],
            held: vec![(0, FIX), (3, FIX)],
            loads: vec![Load::Point {
                beam: 1,
                at: w / 2.0,
                force: V3::new(0.0, 0.0, -10_000.0),
                moment: NO_LOAD,
            }],
            ..Default::default()
        };
        let s = m.solve_static(11).unwrap();
        let sum: Real = s.reactions.iter().map(|r| r[2]).sum();
        close(sum, 10_000.0, 1e-9, "vertical equilibrium");
        close(s.reactions[0][2], s.reactions[1][2], 1e-9, "symmetry");
        close(s.reactions[0][0], -s.reactions[1][0], 1e-9, "thrust");
        assert!((s.nodes[1][2] - s.nodes[2][2]).abs() < 1e-9);
        // Moment about the base point of the first leg is balanced too.
        let mz: Real =
            s.reactions[0][4] + s.reactions[1][4] + (w * s.reactions[1][2]) + (w * -10_000.0 / 2.0);
        assert!(mz.abs() < 1e-3, "{mz}");
    }

    /// A rigid link carries a tip load across a gap: the far beam's root is
    /// offset, so its end moves with the rotation of the first.
    #[test]
    fn a_rigid_link_carries_the_load() {
        let l = 1000.0;
        let m = Model {
            nodes: vec![
                V3::zeros(),
                V3::new(l, 0.0, 0.0),
                V3::new(l + 100.0, 0.0, 0.0),
                V3::new(2.0 * l + 100.0, 0.0, 0.0),
            ],
            beams: vec![beam(0, 1, V3::y()), beam(2, 3, V3::y())],
            held: vec![(0, FIX)],
            links: vec![RigidLink {
                parent: 1,
                child: 2,
            }],
            loads: vec![Load::Node {
                node: 3,
                force: V3::new(0.0, 0.0, -100.0),
                moment: NO_LOAD,
            }],
        };
        let s = m.solve_static(2).unwrap();
        // The first beam has a tip shear P and a tip moment P·(L + 100).
        let ei = E * 2.0e6;
        let (p, c) = (-100.0, l + 100.0);
        let tip = p * l.powi(3) / (3.0 * ei) + p * c * l * l / (2.0 * ei);
        close(s.nodes[1][2], tip, 1e-9, "tip of the first beam");
        let slope = p * l * l / (2.0 * ei) + p * c * l / ei;
        let far = tip + slope * 100.0 + slope * l + p * l.powi(3) / (3.0 * ei);
        close(s.nodes[3][2], far, 1e-9, "far end");
    }

    /// Gravity on a cantilever: ρAgL²/2 at the root.
    #[test]
    fn gravity_loads_every_beam() {
        let l = 1200.0;
        let g = 9806.65;
        let mut m = along_x(l);
        m.held.push((0, FIX));
        m.loads.push(Load::Gravity {
            g: V3::new(0.0, 0.0, -g),
        });
        let s = m.solve_static(2).unwrap();
        let w = steel().rho * section().area * g;
        close(s.beams[0][0].mu.abs(), w * l * l / 2.0, 1e-9, "root moment");
        close(s.reactions[0][2], w * l, 1e-9, "reaction");
    }

    /// Modes of a simply supported beam: fₙ = n²π/(2L²)·√(EI/ρA).
    #[test]
    fn natural_frequencies_of_a_beam() {
        let l = 2000.0;
        let mut m = along_x(l);
        m.held.push((0, [true, true, true, true, false, false]));
        m.held.push((1, [false, true, true, false, false, false]));
        let modes = m.solve_modes(4, 32).unwrap();
        let c = section();
        let st = steel();
        let base = std::f64::consts::PI / (2.0 * l * l);
        // Bending about v (the smaller inertia) comes first.
        let f1 = base * (E * c.iv / (st.rho * c.area)).sqrt();
        close(modes[0].hz, f1, 2e-3, "first mode");
        // About u (4× the inertia): twice that frequency; n = 2 about v: 4×.
        close(modes[1].hz, 2.0 * f1, 2e-3, "second");
    }

    /// A free model starts with six rigid modes.
    #[test]
    fn a_free_model_has_six_rigid_modes() {
        let m = along_x(1000.0);
        let modes = m.solve_modes(8, 16).unwrap();
        for md in &modes[..6] {
            assert!(md.hz < 1e-2 * modes[6].hz, "{}", md.hz);
        }
        assert!(modes[6].hz > 1.0);
    }
}
