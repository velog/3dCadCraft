//! The assembly solver (`docs/spec/3.1-assemblies.md` §6.2): every
//! occurrence that is not grounded has six unknowns — a translation and a
//! small rotation about its own origin, applied to its current placement
//! and re-linearised each iteration (no gimbal lock). Constraints are a
//! few scalar residuals on their geometry moved into the assembly
//! (`relation::residuals`), each depending on at most two components, so
//! its Jacobian is taken by finite differences over those twelve unknowns
//! only.
//!
//! Large assemblies (1000 components) solve in passes:
//! 1. **along the graph**: from the fixed components (grounded, the
//!    assembly's origin) outwards, each component is solved alone against
//!    the constraints to components already placed — a chain or a tree is
//!    done in one sweep;
//! 2. **loops**: constraints still unmet (a four-bar linkage) are solved
//!    together over the components they connect;
//! 3. **limits**: a value past its limit is held there and the passes run
//!    again.
//!
//! Steps are Levenberg in minimum-norm form (`dx = −Jᵀ(JJᵀ + λI)⁻¹r`), so
//! components move as little as the constraints allow. A **drag** pulls a
//! point of one component towards the cursor (a soft goal), then the result
//! is projected back onto the constraints exactly: a constrained component
//! moves only along its free directions, and its partners follow.

use std::collections::{HashMap, HashSet, VecDeque};

use nalgebra::{DMatrix, DVector};

use super::relation::{self, Constraint, Geom, GeomRef, residuals};
use crate::document::AssemblyDocument;
use crate::kernel::{Point3, Real, Transform3, Vector3};

/// The result of a solve.
#[derive(Clone, Debug, Default)]
pub struct Solved {
    /// New placements of the components that moved.
    pub transforms: Vec<(u32, Transform3)>,
    /// Constraints whose geometry could not be found: (id, reason).
    pub sick: Vec<(u32, String)>,
    /// Every active constraint holds (to 1e-6).
    pub converged: bool,
    /// The largest remaining residual.
    pub residual: Real,
    /// Every residual of the active relationships at the result, in their
    /// order (Adaptivity minimises them over the part's free sizes).
    pub residuals: Vec<Real>,
    /// Free degrees of freedom of each component that is not grounded
    /// (the others held where they are).
    pub dof: HashMap<u32, usize>,
}

/// A drag: the point `local` of component `occurrence` (its own
/// coordinates) should come to `target` (assembly coordinates).
#[derive(Clone, Copy, Debug)]
pub struct Drag {
    /// The dragged component.
    pub occurrence: u32,
    /// The grabbed point, in the component's coordinates.
    pub local: Point3,
    /// Where the cursor puts it.
    pub target: Point3,
}

/// Length scale of the rotation unknowns: a turn of 1/L radian counts like
/// a move of 1 mm.
const L: Real = 500.0;
/// Weight of the drag goal against the constraints.
const GOAL: Real = 1e-2;
const TOL: Real = 1e-10;

/// The components a constraint reads (two, or three with a plane).
#[derive(Clone, Copy)]
struct Ends([u32; 3], usize);

impl std::ops::Deref for Ends {
    type Target = [u32];
    fn deref(&self) -> &[u32] {
        &self.0[..self.1]
    }
}

impl<'a> IntoIterator for &'a Ends {
    type Item = &'a u32;
    type IntoIter = std::slice::Iter<'a, u32>;
    fn into_iter(self) -> Self::IntoIter {
        self.0[..self.1].iter()
    }
}

impl IntoIterator for Ends {
    type Item = u32;
    type IntoIter = std::iter::Take<std::array::IntoIter<u32, 3>>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter().take(self.1)
    }
}

/// One active constraint with its local geometry (and Symmetry's plane).
struct Con {
    c: Constraint,
    a: Geom,
    b: Geom,
    s: Option<(u32, Geom)>,
}

impl Con {
    /// The components it reads: its two selections' and the plane's. No
    /// allocation: it runs for every constraint of every component in the
    /// passes (a `Vec` here made 1000 parts take 190 ms instead of 45).
    fn ends(&self) -> Ends {
        let (a, b) = (self.c.a.occurrence(), self.c.b.occurrence());
        match &self.s {
            Some((id, _)) => Ends([a, b, *id], 3),
            None => Ends([a, b, 0], 2),
        }
    }

    /// Its residuals with the placements `ts` (in `ends` order).
    fn res_ts(&self, ts: &[Transform3], out: &mut Vec<Real>) {
        let (a, b) = (self.a.moved(&ts[0]), self.b.moved(&ts[1]));
        match (&self.s, ts.get(2)) {
            (Some((_, s)), Some(t)) => {
                let _ = relation::residuals_sym(&self.c, &a, &b, &s.moved(t), out);
            }
            _ => {
                let _ = residuals(&self.c, &a, &b, out);
            }
        }
    }

    fn res(&self, place: &Place, out: &mut Vec<Real>) {
        let e = self.ends();
        let mut ts = [Transform3::identity(); 3];
        for (t, &id) in ts.iter_mut().zip(e.iter()) {
            *t = at(place, id);
        }
        self.res_ts(&ts[..e.len()], out);
    }

    /// Ready to solve for `id` when every other component it reads is
    /// fixed or placed already.
    fn ready_for(&self, id: u32, free: &HashSet<u32>, placed: &HashSet<u32>) -> bool {
        let e = self.ends();
        e.contains(&id)
            && e.iter()
                .all(|x| *x == id || !free.contains(x) || placed.contains(x))
    }
}

/// Moves a placement by six unknowns (translation, turn about its own
/// origin scaled by `L`).
fn nudge(t: &Transform3, v: &[Real]) -> Transform3 {
    let c = t.translation.vector;
    Transform3::translation(v[0], v[1], v[2])
        * Transform3::translation(c.x, c.y, c.z)
        * Transform3::rotation(Vector3::new(v[3], v[4], v[5]) / L)
        * Transform3::translation(-c.x, -c.y, -c.z)
        * t
}

type Place = HashMap<u32, Transform3>;

fn at(place: &Place, id: u32) -> Transform3 {
    place.get(&id).copied().unwrap_or_else(Transform3::identity)
}

/// The rows of one residual source and their derivatives by the unknowns of
/// its components that are in `col` (finite differences).
fn rows(
    place: &Place,
    ids: &[u32],
    col: &HashMap<u32, usize>,
    f: &dyn Fn(&[Transform3], &mut Vec<Real>),
    j: &mut Vec<(usize, usize, Real)>,
    r: &mut Vec<Real>,
) {
    let ts: Vec<Transform3> = ids.iter().map(|&id| at(place, id)).collect();
    let row0 = r.len();
    let mut base = Vec::new();
    f(&ts, &mut base);
    let h = 1e-7;
    for (k, &id) in ids.iter().enumerate() {
        let Some(&c) = col.get(&id) else {
            continue;
        };
        // Both ends one component: differentiate it once.
        if ids[..k].contains(&id) {
            continue;
        }
        for i in 0..6 {
            let mut v = [0.0; 6];
            v[i] = h;
            let moved = nudge(&ts[k], &v);
            let tt: Vec<Transform3> = ids
                .iter()
                .zip(&ts)
                .map(|(&x, t)| if x == id { moved } else { *t })
                .collect();
            let mut out = Vec::new();
            f(&tt, &mut out);
            for (q, (x, y)) in out.iter().zip(&base).enumerate() {
                let d = (x - y) / h;
                if d != 0.0 {
                    j.push((row0 + q, 6 * c + i, d));
                }
            }
        }
    }
    r.extend(base);
}

/// Levenberg on the components `set` for the constraints `cons`, with an
/// optional drag goal. Returns the largest constraint residual left.
fn lm(cons: &[&Con], set: &[u32], place: &mut Place, drag: Option<&Drag>, iters: usize) -> Real {
    let col: HashMap<u32, usize> = set.iter().enumerate().map(|(k, &b)| (b, k)).collect();
    let n = set.len() * 6;
    let eval = |place: &Place| -> (Vec<Real>, Vec<(usize, usize, Real)>, usize) {
        let (mut r, mut j) = (Vec::new(), Vec::new());
        for k in cons {
            rows(
                place,
                &k.ends(),
                &col,
                &|t, o| k.res_ts(t, o),
                &mut j,
                &mut r,
            );
        }
        let m = r.len();
        if let Some(d) = drag {
            rows(
                place,
                &[d.occurrence],
                &col,
                &|t, o| {
                    let w = t[0] * d.local - d.target;
                    o.extend([w.x * GOAL, w.y * GOAL, w.z * GOAL]);
                },
                &mut j,
                &mut r,
            );
        }
        (r, j, m)
    };
    let worst = |r: &[Real], m: usize| r.iter().take(m).fold(0.0, |x: Real, v| x.max(v.abs()));
    let sq = |r: &[Real]| r.iter().map(|v| v * v).sum::<Real>();
    let (mut r, mut j, mut m) = eval(place);
    if n == 0 {
        return worst(&r, m);
    }
    let mut lambda = 1e-6;
    for _ in 0..iters {
        if r.iter().fold(0.0, |x: Real, v| x.max(v.abs())) < TOL {
            break;
        }
        let mut jm = DMatrix::zeros(r.len(), n);
        for &(a, b, v) in &j {
            jm[(a, b)] = v;
        }
        let rv = DVector::from_column_slice(&r);
        let jjt = &jm * jm.transpose();
        let mut improved = false;
        for _ in 0..12 {
            let a = &jjt + DMatrix::identity(r.len(), r.len()) * lambda;
            let Some(y) = a.lu().solve(&rv) else {
                lambda *= 10.0;
                continue;
            };
            let dx = -(jm.transpose() * y);
            let old: Vec<(u32, Transform3)> = set.iter().map(|&id| (id, at(place, id))).collect();
            for (k, (id, t)) in old.iter().enumerate() {
                place.insert(*id, nudge(t, &dx.as_slice()[6 * k..6 * k + 6]));
            }
            let (r1, j1, m1) = eval(place);
            if sq(&r1) < sq(&r) {
                (r, j, m) = (r1, j1, m1);
                lambda = (lambda / 3.0).max(1e-12);
                improved = true;
                break;
            }
            for (id, t) in old {
                place.insert(id, t);
            }
            lambda *= 4.0;
        }
        if !improved {
            break;
        }
    }
    worst(&r, m)
}

/// The passes over the whole assembly (see the module docs).
fn passes(cons: &[Con], free: &HashSet<u32>, place: &mut Place, order: &[u32]) -> Real {
    // The constraints of each component (scanning all of them for every
    // component was quadratic).
    let mut of: HashMap<u32, Vec<usize>> = HashMap::new();
    for (i, k) in cons.iter().enumerate() {
        for id in k.ends() {
            let v = of.entry(id).or_default();
            if v.last() != Some(&i) {
                v.push(i);
            }
        }
    }
    let none: Vec<usize> = Vec::new();
    // 1. Along the graph: each component against the ones placed before it.
    let mut placed: HashSet<u32> = HashSet::new();
    for &id in order {
        let mine: Vec<&Con> = of
            .get(&id)
            .unwrap_or(&none)
            .iter()
            .map(|&i| &cons[i])
            .filter(|k| k.ready_for(id, free, &placed))
            .collect();
        if !mine.is_empty() {
            lm(&mine, &[id], place, None, 60);
        }
        placed.insert(id);
    }
    // 2. Loops: what is still unmet, over the components it connects.
    let unmet = |place: &Place| -> Vec<usize> {
        cons.iter()
            .enumerate()
            .filter(|(_, k)| {
                let mut r = Vec::new();
                k.res(place, &mut r);
                r.iter().any(|v| v.abs() > 1e-7)
            })
            .map(|(i, _)| i)
            .collect()
    };
    let bad = unmet(place);
    if !bad.is_empty() {
        // The free components joined (through free components) to them.
        let mut set: Vec<u32> = Vec::new();
        let mut queue: VecDeque<u32> = bad
            .iter()
            .flat_map(|&i| cons[i].ends())
            .filter(|id| free.contains(id))
            .collect();
        let mut seen: HashSet<u32> = HashSet::new();
        while let Some(id) = queue.pop_front() {
            if !seen.insert(id) {
                continue;
            }
            set.push(id);
            for &i in of.get(&id).unwrap_or(&none) {
                for y in cons[i].ends() {
                    if free.contains(&y) && !seen.contains(&y) {
                        queue.push_back(y);
                    }
                }
            }
        }
        let mine: Vec<&Con> = cons
            .iter()
            .filter(|k| k.ends().iter().any(|e| seen.contains(e)))
            .collect();
        lm(&mine, &set, place, None, 200);
    }
    cons.iter().fold(0.0, |m: Real, k| {
        let mut r = Vec::new();
        k.res(place, &mut r);
        r.iter().fold(m, |m, v| m.max(v.abs()))
    })
}

/// The free components in the order of the graph: breadth first from the
/// fixed ones (and the assembly's origin), the rest after them.
/// An island without a fixed component starts from `first` (the dragged
/// one) when it is there, so the others follow it.
fn graph_order(cons: &[Con], free: &HashSet<u32>, blocks: &[u32], first: Option<u32>) -> Vec<u32> {
    let mut adj: HashMap<u32, Vec<u32>> = HashMap::new();
    for k in cons {
        let e = k.ends();
        for &x in &e {
            for &y in &e {
                if x != y {
                    adj.entry(x).or_default().push(y);
                }
            }
        }
    }
    let mut order = Vec::new();
    let mut seen: HashSet<u32> = HashSet::new();
    let mut queue: VecDeque<u32> = VecDeque::new();
    // Start from every fixed component that has constraints.
    let mut fixed: Vec<u32> = adj
        .keys()
        .copied()
        .filter(|id| !free.contains(id))
        .collect();
    fixed.sort_unstable();
    queue.extend(fixed);
    let visit = |queue: &mut VecDeque<u32>, order: &mut Vec<u32>, seen: &mut HashSet<u32>| {
        while let Some(id) = queue.pop_front() {
            for &n in adj.get(&id).map(Vec::as_slice).unwrap_or(&[]) {
                if free.contains(&n) && seen.insert(n) {
                    order.push(n);
                    queue.push_back(n);
                }
            }
        }
    };
    visit(&mut queue, &mut order, &mut seen);
    // Islands with no fixed component: from their first block.
    for &b in first.iter().chain(blocks) {
        if seen.insert(b) {
            order.push(b);
            queue.push_back(b);
            visit(&mut queue, &mut order, &mut seen);
        }
    }
    order
}

/// The active relationships with their geometry; the ones whose geometry
/// cannot be found go to `sick`.
fn gather(
    asm: &AssemblyDocument,
    local: &dyn Fn(&GeomRef) -> Result<Geom, String>,
    drag: Option<&Drag>,
    sick: &mut Vec<(u32, String)>,
) -> Vec<Con> {
    let mut cons: Vec<Con> = Vec::new();
    // A suppressed component's relationships rest (not sick).
    let off =
        |c: &relation::Constraint| asm.occurrences.iter().any(|o| o.suppressed && c.uses(o.id));
    for c in asm.constraints.iter().filter(|c| !c.suppressed && !off(c)) {
        // Symmetry's plane.
        let plane = match &c.plane {
            Some(r) => match local(r) {
                Ok(g) => Some((r.occurrence(), g)),
                Err(e) => {
                    sick.push((c.id, e));
                    continue;
                }
            },
            None => None,
        };
        match (local(&c.a), local(&c.b)) {
            (Ok(a), Ok(b)) => {
                let mut c = c.clone();
                // At rest unless dragged (Use Offset As Resting Position).
                if c.resting && drag.is_none() {
                    c.locked = true;
                }
                let (a, b) = if c.kind.is_joint() {
                    (a.as_frame(), b.as_frame())
                } else {
                    (a, b)
                };
                cons.push(Con { c, a, b, s: plane });
            }
            (Err(e), _) | (_, Err(e)) => sick.push((c.id, e)),
        }
    }
    cons
}

/// Every residual of the active relationships at the components' current
/// placements (no solve), and how many are sick. Adaptivity
/// (`assembly::adapt`) solves placements and part sizes together on them.
pub fn residuals_at(
    asm: &AssemblyDocument,
    local: &dyn Fn(&GeomRef) -> Result<Geom, String>,
) -> (Vec<Real>, usize) {
    let mut sick = Vec::new();
    let cons = gather(asm, local, None, &mut sick);
    let mut place: Place = asm
        .occurrences
        .iter()
        .map(|o| (o.id, o.transform))
        .collect();
    place.insert(relation::ASSEMBLY, Transform3::identity());
    let mut r = Vec::new();
    for k in &cons {
        k.res(&place, &mut r);
    }
    (r, sick.len())
}

/// The components that are not grounded joined to `start` through active
/// relationships between such components (`start` first).
pub fn free_group(asm: &AssemblyDocument, start: u32) -> Vec<u32> {
    let free =
        |id: u32| asm.find(id).is_some_and(|o| !o.grounded && !o.suppressed) && !asm.is_element(id);
    let mut out = Vec::new();
    let mut queue = VecDeque::from([start]);
    while let Some(id) = queue.pop_front() {
        if out.contains(&id) || !free(id) {
            continue;
        }
        out.push(id);
        for c in asm
            .constraints
            .iter()
            .filter(|c| !c.suppressed && c.uses(id))
        {
            for y in [c.a.occurrence(), c.b.occurrence()] {
                if !out.contains(&y) {
                    queue.push_back(y);
                }
            }
        }
    }
    out
}

/// Solves the constraints of `asm`. `local` gives a reference's geometry in
/// its part's coordinates; `drag` pulls a point of one component.
pub fn solve(
    asm: &AssemblyDocument,
    local: &dyn Fn(&GeomRef) -> Result<Geom, String>,
    drag: Option<&Drag>,
) -> Solved {
    let mut out = Solved::default();
    let mut cons = gather(asm, local, drag, &mut out.sick);
    // Free components: used by a constraint, not grounded.
    let mut blocks: Vec<u32> = Vec::new();
    for k in &cons {
        for id in k.ends() {
            // A pattern's copy is placed by its pattern, never solved.
            let free = asm.find(id).is_some_and(|o| !o.grounded) && !asm.is_element(id);
            if free && !blocks.contains(&id) {
                blocks.push(id);
            }
        }
    }
    let free: HashSet<u32> = blocks.iter().copied().collect();
    let mut place: Place = asm
        .occurrences
        .iter()
        .map(|o| (o.id, o.transform))
        .collect();
    place.insert(relation::ASSEMBLY, Transform3::identity());
    // A drag first pulls the dragged component's group, softly.
    if let Some(d) = drag
        && free.contains(&d.occurrence)
    {
        let group = connected(&cons, &free, d.occurrence);
        let mine: Vec<&Con> = cons
            .iter()
            .filter(|k| k.ends().iter().any(|e| group.contains(e)))
            .collect();
        lm(&mine, &group, &mut place, Some(d), 40);
    }
    let order = graph_order(&cons, &free, &blocks, drag.map(|d| d.occurrence));
    let mut worst = passes(&cons, &free, &mut place, &order);
    // Limits: a value past one is held there; then solve again.
    for _ in 0..3 {
        let mut held = false;
        for k in cons.iter_mut() {
            if k.c.locked || (k.c.min.is_none() && k.c.max.is_none()) {
                continue;
            }
            let (a, b) = (k.c.a.occurrence(), k.c.b.occurrence());
            let (ga, gb) = (k.a.moved(&at(&place, a)), k.b.moved(&at(&place, b)));
            let Some(v) = relation::measure(&k.c, &ga, &gb) else {
                continue;
            };
            let lo = k.c.min.unwrap_or(Real::NEG_INFINITY);
            let hi = k.c.max.unwrap_or(Real::INFINITY);
            if v < lo - 1e-9 || v > hi + 1e-9 {
                k.c.locked = true;
                k.c.value = v.clamp(lo, hi);
                held = true;
            }
        }
        if !held {
            break;
        }
        worst = passes(&cons, &free, &mut place, &order);
    }
    out.residual = worst;
    out.converged = worst < 1e-6;
    // Degrees of freedom of each block, the others held (not while
    // dragging: nobody reads them then).
    if drag.is_none() {
        for &id in &blocks {
            let mine: Vec<&Con> = cons.iter().filter(|k| k.ends().contains(&id)).collect();
            let col: HashMap<u32, usize> = [(id, 0)].into_iter().collect();
            let (mut r, mut j) = (Vec::new(), Vec::new());
            for k in &mine {
                rows(
                    &place,
                    &k.ends(),
                    &col,
                    &|t, o| k.res_ts(t, o),
                    &mut j,
                    &mut r,
                );
            }
            let mut m = DMatrix::zeros(r.len(), 6);
            for (a, b, v) in j {
                m[(a, b)] = v;
            }
            let rank = if r.is_empty() {
                0
            } else {
                let sv = m.svd(false, false).singular_values;
                let top = sv.iter().cloned().fold(0.0, Real::max);
                sv.iter().filter(|&&s| s > 1e-6 * top.max(1e-9)).count()
            };
            out.dof.insert(id, 6 - rank.min(6));
        }
    }
    for o in &asm.occurrences {
        if !o.grounded && !out.dof.contains_key(&o.id) {
            out.dof.insert(o.id, 6);
        }
    }
    for k in &cons {
        k.res(&place, &mut out.residuals);
    }
    out.transforms = blocks.iter().map(|id| (*id, at(&place, *id))).collect();
    out
}

/// The free components joined to `start` through constraints between free
/// components.
fn connected(cons: &[Con], free: &HashSet<u32>, start: u32) -> Vec<u32> {
    let mut seen: HashSet<u32> = HashSet::new();
    let mut out = Vec::new();
    let mut queue = VecDeque::from([start]);
    while let Some(id) = queue.pop_front() {
        if !seen.insert(id) {
            continue;
        }
        out.push(id);
        for k in cons {
            let e = k.ends();
            if e.contains(&id) {
                for &y in &e {
                    if free.contains(&y) && !seen.contains(&y) {
                        queue.push_back(y);
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assembly::relation::{Constraint, ConstraintKind};
    use crate::document::{DocRef, DocumentId};
    use crate::kernel::Point3;

    /// Two blocks: geometry given directly (part coordinates): a 20 mm cube.
    fn cube_geom(r: &GeomRef) -> Result<Geom, String> {
        if let GeomRef::Origin { item, .. } = r {
            return Ok(item.geom());
        }
        let GeomRef::Face { name, .. } = r else {
            return circle_geom(r);
        };
        if name == "Shaft" {
            // A Ø10 shaft along the cube's z through its middle.
            return Ok(Geom::Cylinder {
                p: Point3::new(10.0, 10.0, 0.0),
                d: Vector3::z(),
                r: 5.0,
            });
        }
        let (p, n) = match name.as_str() {
            "Top" => (Point3::new(10.0, 10.0, 20.0), Vector3::z()),
            "Bottom" => (Point3::new(10.0, 10.0, 0.0), -Vector3::z()),
            "Right" => (Point3::new(20.0, 10.0, 10.0), Vector3::x()),
            "Left" => (Point3::new(0.0, 10.0, 10.0), -Vector3::x()),
            "Front" => (Point3::new(10.0, 0.0, 10.0), -Vector3::y()),
            "Back" => (Point3::new(10.0, 20.0, 10.0), Vector3::y()),
            _ => return Err(format!("the face {name} no longer exists")),
        };
        Ok(Geom::Plane { p, n })
    }

    /// A hole's rim on the top (Ø6 at the centre).
    fn circle_geom(_r: &GeomRef) -> Result<Geom, String> {
        Ok(Geom::Circle {
            c: Point3::new(10.0, 10.0, 20.0),
            n: Vector3::z(),
            r: 3.0,
        })
    }

    fn face(o: u32, name: &str) -> GeomRef {
        GeomRef::Face {
            sub: Vec::new(),
            occurrence: o,
            name: name.into(),
            hint: [0.0; 3],
        }
    }

    fn rim(o: u32) -> GeomRef {
        GeomRef::Edge {
            sub: Vec::new(),
            occurrence: o,
            edge: crate::part::EdgeRef {
                faces: ["Top".into(), "Hole".into()],
                hint: [0.0; 3],
            },
        }
    }

    fn two_cubes() -> AssemblyDocument {
        let mut a = AssemblyDocument::default();
        let r = DocRef::local(DocumentId::new_random());
        a.place(r, "Cube", Transform3::identity(), true);
        // The second one somewhere, turned a little.
        a.place(
            r,
            "Cube",
            Transform3::new(Vector3::new(50.0, 30.0, 7.0), Vector3::new(0.2, -0.1, 0.3)),
            false,
        );
        a
    }

    fn con(kind: ConstraintKind, a: GeomRef, b: GeomRef, value: Real) -> Constraint {
        Constraint {
            id: 0,
            name: String::new(),
            kind,
            a,
            b,
            value,
            expr: None,
            aligned: false,
            suppressed: false,
            locked: false,
            min: None,
            max: None,
            resting: false,
            plane: None,
        }
    }

    fn applied(a: &mut AssemblyDocument, s: &Solved) {
        for (id, t) in &s.transforms {
            if let Some(o) = a.find_mut(*id) {
                o.transform = *t;
            }
        }
    }

    /// A suppressed component's relationships rest: it is not moved and
    /// they are not sick (Inventor).
    #[test]
    fn suppressed_components_rest() {
        let mut a = two_cubes();
        a.add_constraint(con(
            ConstraintKind::Mate,
            face(1, "Top"),
            face(2, "Bottom"),
            0.0,
        ));
        a.find_mut(2).unwrap().suppressed = true;
        let s = solve(&a, &cube_geom, None);
        assert!(s.converged && s.sick.is_empty());
        assert!(!s.transforms.iter().any(|(i, _)| *i == 2));
        a.find_mut(2).unwrap().suppressed = false;
        let s = solve(&a, &cube_geom, None);
        assert!(s.transforms.iter().any(|(i, _)| *i == 2));
    }

    #[test]
    fn mate_leaves_three_freedoms_and_three_mates_none() {
        let mut a = two_cubes();
        a.add_constraint(con(
            ConstraintKind::Mate,
            face(1, "Top"),
            face(2, "Bottom"),
            0.0,
        ));
        let s = solve(&a, &cube_geom, None);
        assert!(s.converged, "{}", s.residual);
        assert_eq!(s.dof[&2], 3, "two moves in the plane, one turn");
        applied(&mut a, &s);
        // Cube 2's bottom now lies on cube 1's top (z = 20), facing down.
        let t = a.find(2).unwrap().transform;
        let bottom = t * Point3::new(10.0, 10.0, 0.0);
        assert!((bottom.z - 20.0).abs() < 1e-6, "{bottom}");
        assert!(((t * -Vector3::z()) + Vector3::z()).norm() < 1e-6);
        // Flush sides: left on left, front on front.
        a.add_constraint(con(
            ConstraintKind::Flush,
            face(1, "Left"),
            face(2, "Left"),
            0.0,
        ));
        a.add_constraint(con(
            ConstraintKind::Flush,
            face(1, "Front"),
            face(2, "Front"),
            5.0,
        ));
        let s = solve(&a, &cube_geom, None);
        assert!(s.converged, "{}", s.residual);
        assert_eq!(s.dof[&2], 0);
        applied(&mut a, &s);
        let t = a.find(2).unwrap().transform;
        let corner = t * Point3::origin();
        // Front flush at offset 5 along the front's normal (−y): y = −5.
        assert!(
            (corner - Point3::new(0.0, -5.0, 20.0)).norm() < 1e-6,
            "{corner}"
        );
        assert_eq!(a.constraints[1].name, "Flush:1");
    }

    #[test]
    fn insert_keeps_the_turn_free_and_conflicts_do_not_converge() {
        let mut a = two_cubes();
        a.add_constraint(Constraint {
            aligned: false,
            ..con(ConstraintKind::Insert, rim(1), rim(2), 0.0)
        });
        let s = solve(&a, &cube_geom, None);
        assert!(s.converged, "{}", s.residual);
        assert_eq!(s.dof[&2], 1, "the turn about the axis");
        applied(&mut a, &s);
        let t = a.find(2).unwrap().transform;
        // Opposed: cube 2's rim normal points down onto cube 1's rim.
        let c = t * Point3::new(10.0, 10.0, 20.0);
        assert!((c - Point3::new(10.0, 10.0, 20.0)).norm() < 1e-6, "{c}");
        // A Mate of the same faces at another offset cannot hold with it.
        a.add_constraint(con(
            ConstraintKind::Mate,
            face(1, "Top"),
            face(2, "Top"),
            3.0,
        ));
        let s = solve(&a, &cube_geom, None);
        assert!(!s.converged);
    }

    #[test]
    fn angle_and_missing_geometry() {
        let mut a = two_cubes();
        a.add_constraint(con(
            ConstraintKind::Mate,
            face(1, "Top"),
            face(2, "Bottom"),
            0.0,
        ));
        a.add_constraint(con(
            ConstraintKind::Angle,
            face(1, "Right"),
            face(2, "Right"),
            30f64.to_radians(),
        ));
        a.add_constraint(con(
            ConstraintKind::Mate,
            face(1, "Gone"),
            face(2, "Left"),
            0.0,
        ));
        let s = solve(&a, &cube_geom, None);
        assert!(s.converged, "{}", s.residual);
        assert_eq!(s.sick.len(), 1);
        assert!(s.sick[0].1.contains("Gone"));
        assert_eq!(s.dof[&2], 2);
        applied(&mut a, &s);
        let t = a.find(2).unwrap().transform;
        let cos = (t * Vector3::x()).dot(&Vector3::x());
        assert!((cos - 30f64.to_radians().cos()).abs() < 1e-6);
    }

    /// Angles at the ends of their range (0, 180 degrees and close to
    /// them): `cos θ − cos d` has a double root there, so the solver
    /// stalled 7.5e-5 rad short and the DOF stayed open.
    #[test]
    fn angles_at_the_ends_of_their_range() {
        for deg in [0.0, 180.0, 1e-4, 179.99, 0.01, 90.0] {
            let d: Real = deg;
            let mut a = two_cubes();
            a.add_constraint(con(
                ConstraintKind::Mate,
                face(1, "Top"),
                face(2, "Bottom"),
                0.0,
            ));
            a.add_constraint(con(
                ConstraintKind::Angle,
                face(1, "Right"),
                face(2, "Right"),
                d.to_radians(),
            ));
            let s = solve(&a, &cube_geom, None);
            assert!(s.converged, "{deg}: {}", s.residual);
            assert_eq!(s.dof[&2], 2, "{deg}");
            applied(&mut a, &s);
            let t = a.find(2).unwrap().transform;
            let u = t * Vector3::x();
            let got = u.cross(&Vector3::x()).norm().atan2(u.dot(&Vector3::x()));
            assert!(
                (got - d.to_radians()).abs() < 1e-9,
                "{deg}: {} deg",
                got.to_degrees()
            );
        }
    }

    /// The origin planes of a component flush with the assembly's own
    /// (occurrence `ASSEMBLY`): it lands on the origin, square to it.
    #[test]
    fn origin_planes_flush_with_the_assembly_origin() {
        use crate::assembly::relation::{ASSEMBLY, OriginItem};
        let mut a = two_cubes();
        let o = |occurrence, item| GeomRef::Origin {
            sub: Vec::new(),
            occurrence,
            item,
        };
        for item in [
            OriginItem::XyPlane,
            OriginItem::XzPlane,
            OriginItem::YzPlane,
        ] {
            a.add_constraint(con(
                ConstraintKind::Flush,
                o(ASSEMBLY, item),
                o(2, item),
                0.0,
            ));
        }
        let s = solve(&a, &cube_geom, None);
        assert!(s.converged, "{}", s.residual);
        assert_eq!(s.dof[&2], 0);
        applied(&mut a, &s);
        let t = a.find(2).unwrap().transform;
        assert!((t * Point3::new(1.0, 2.0, 3.0) - Point3::new(1.0, 2.0, 3.0)).norm() < 1e-6);
        // A component's centre point on the assembly's Z axis: a Mate of a
        // point and an axis.
        let mut a = two_cubes();
        a.add_constraint(con(
            ConstraintKind::Mate,
            o(ASSEMBLY, OriginItem::ZAxis),
            o(2, OriginItem::Center),
            0.0,
        ));
        let s = solve(&a, &cube_geom, None);
        assert!(s.converged);
        assert_eq!(s.dof[&2], 4, "it slides along Z and turns freely");
        applied(&mut a, &s);
        let c = a.find(2).unwrap().transform * Point3::origin();
        assert!(c.x.abs() < 1e-6 && c.y.abs() < 1e-6, "{c}");
    }

    /// A Mate with limits 0..10: dragged 30 up it stops at 10; dropped there
    /// it stays (no resting position); with Use Offset As Resting Position
    /// it springs back to its offset when solved without a drag.
    #[test]
    fn mate_limits_stop_a_drag() {
        let mut a = two_cubes();
        let mut c = con(ConstraintKind::Mate, face(1, "Top"), face(2, "Bottom"), 0.0);
        c.min = Some(0.0);
        c.max = Some(10.0);
        a.add_constraint(c.clone());
        a.add_constraint(con(
            ConstraintKind::Flush,
            face(1, "Left"),
            face(2, "Left"),
            0.0,
        ));
        a.add_constraint(con(
            ConstraintKind::Flush,
            face(1, "Front"),
            face(2, "Front"),
            0.0,
        ));
        let s = solve(&a, &cube_geom, None);
        assert!(s.converged, "{}", s.residual);
        applied(&mut a, &s);
        let t = a.find(2).unwrap().transform;
        let p = Point3::new(10.0, 10.0, 0.0);
        let drag = Drag {
            occurrence: 2,
            local: p,
            target: t * p + Vector3::new(0.0, 0.0, 30.0),
        };
        let s = solve(&a, &cube_geom, Some(&drag));
        assert!(s.converged, "{}", s.residual);
        applied(&mut a, &s);
        let z = (a.find(2).unwrap().transform * p).z;
        assert!((z - 30.0).abs() < 1e-6, "stopped at the limit: {z}");
        // Dropped: solved again without a drag it stays at 10 above.
        let s = solve(&a, &cube_geom, None);
        applied(&mut a, &s);
        assert!(((a.find(2).unwrap().transform * p).z - 30.0).abs() < 1e-6);
        // Resting position: back to the offset 0.
        a.constraints[0].resting = true;
        let s = solve(&a, &cube_geom, None);
        applied(&mut a, &s);
        assert!(((a.find(2).unwrap().transform * p).z - 20.0).abs() < 1e-6);
    }

    /// A component left free to turn (Insert) turns when a point of it is
    /// dragged sideways: the grabbed point moves round the axis.
    #[test]
    fn dragging_turns_a_component_about_its_free_axis() {
        let mut a = two_cubes();
        a.add_constraint(con(ConstraintKind::Insert, rim(1), rim(2), 0.0));
        let s = solve(&a, &cube_geom, None);
        applied(&mut a, &s);
        let t = a.find(2).unwrap().transform;
        // A corner of the second cube, pulled sideways.
        let p = Point3::new(20.0, 10.0, 10.0);
        let w0 = t * p;
        let drag = Drag {
            occurrence: 2,
            local: p,
            target: w0 + Vector3::new(0.0, 8.0, 0.0),
        };
        let s = solve(&a, &cube_geom, Some(&drag));
        assert!(s.converged, "{}", s.residual);
        applied(&mut a, &s);
        let w1 = a.find(2).unwrap().transform * p;
        // Same distance from the axis (x = y = 10), turned towards +y.
        let r = |w: Point3| ((w.x - 10.0).powi(2) + (w.y - 10.0).powi(2)).sqrt();
        assert!((r(w0) - r(w1)).abs() < 1e-6, "{w0} {w1}");
        assert!(w1.y - w0.y > 3.0, "turned: {w0} -> {w1}");
    }

    /// Tangent: a shaft lying on a top face (its axis parallel to it, 5 mm
    /// above); Symmetry: a face mirrored about the assembly's YZ plane.
    #[test]
    fn tangent_and_symmetry() {
        use crate::assembly::relation::{ASSEMBLY, OriginItem};
        let mut a = two_cubes();
        a.add_constraint(con(
            ConstraintKind::Tangent,
            face(1, "Top"),
            face(2, "Shaft"),
            0.0,
        ));
        let s = solve(&a, &cube_geom, None);
        assert!(s.converged, "{}", s.residual);
        applied(&mut a, &s);
        let t = a.find(2).unwrap().transform;
        let (p, d) = (t * Point3::new(10.0, 10.0, 0.0), t * Vector3::z());
        assert!(d.z.abs() < 1e-6, "the axis lies flat: {d}");
        assert!((p.z - 25.0).abs() < 1e-6, "5 above the top: {p}");
        // Inside a bigger round would be |r1 - r2|; Outside against the
        // plane from below:
        let mut a = two_cubes();
        let mut c = con(
            ConstraintKind::Symmetry,
            face(1, "Right"),
            face(2, "Left"),
            0.0,
        );
        c.plane = Some(GeomRef::Origin {
            sub: Vec::new(),
            occurrence: ASSEMBLY,
            item: OriginItem::YzPlane,
        });
        a.add_constraint(c);
        let s = solve(&a, &cube_geom, None);
        assert!(s.converged, "{}", s.residual);
        applied(&mut a, &s);
        let t = a.find(2).unwrap().transform;
        // Cube 1's right face (x = 20) mirrored: x = -20, facing -x.
        let left = t * Point3::new(0.0, 10.0, 10.0);
        let n = t * -Vector3::x();
        assert!(
            (left.x + 20.0).abs() < 1e-6 && (n + Vector3::x()).norm() < 1e-6,
            "{left} {n}"
        );
    }

    #[test]
    fn a_dragged_component_pulls_its_partner() {
        // Both free (the first grounded one is freed): mating them and
        // dragging the second moves the first along.
        let mut a = two_cubes();
        a.add_constraint(con(
            ConstraintKind::Mate,
            face(1, "Top"),
            face(2, "Bottom"),
            0.0,
        ));
        let s = solve(&a, &cube_geom, None);
        applied(&mut a, &s);
        // Now the first one is freed too.
        a.find_mut(1).unwrap().grounded = false;
        // Drag cube 2 up by 10: the first follows (it is the light one).
        let before1 = a.find(1).unwrap().transform;
        // The cube's own origin pulled 10 up.
        let t2 = a.find(2).unwrap().transform;
        let z2 = (t2 * Point3::origin()).z + 10.0;
        let drag = Drag {
            occurrence: 2,
            local: Point3::origin(),
            target: t2 * Point3::origin() + Vector3::new(0.0, 0.0, 10.0),
        };
        let s = solve(&a, &cube_geom, Some(&drag));
        assert!(s.converged);
        applied(&mut a, &s);
        let z2_after = (a.find(2).unwrap().transform * Point3::origin()).z;
        let moved1 =
            (a.find(1).unwrap().transform.translation.vector - before1.translation.vector).norm();
        assert!(
            (z2_after - z2).abs() < 0.1,
            "the dragged one stays: {z2} {z2_after}"
        );
        assert!(moved1 > 9.0, "the other follows: {moved1}");
    }
}
