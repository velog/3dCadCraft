//! Adaptivity (spec 3.10 §3.4, stage B): a part whose occurrence is
//! *adaptive* takes its size from the assembly's relationships. Its
//! adaptive sketches' free degrees of freedom (points not fixed by the
//! sketch's own constraints and dimensions, circles of free radius) and its
//! adaptive features'
//! distances are unknowns next to the components' placements: when the
//! rigid solve cannot meet the relationships, those unknowns change until
//! it can (the smallest change that does, Levenberg on the residuals of the
//! solved assembly). The result is written into the part like a user's
//! edit — moved sketch points, a new distance — so the part still
//! regenerates from its own data (rule 8) and keeps its last size opened
//! alone. While a component joined to it is dragged
//! ([`adapt_dragged`]), the part stretches with the drag.

use super::relation::{self, Geom, GeomRef};
use super::solve::{self, Solved};
use crate::document::AssemblyDocument;
use crate::kernel::{Real, Transform3, Vector3};
use crate::part::{Feature, PartDocument, params};
use crate::sketch::{Entity, EntityId, PointFreedom, PointId, Vector2};

/// One unknown of an adaptive part.
#[derive(Clone, Debug, PartialEq)]
pub enum Unknown {
    /// A free sketch point sliding along `dir` (sketch coordinates) from
    /// where it is.
    Point {
        /// Index of the sketch in `PartDocument::sketches`.
        sketch: usize,
        /// The point.
        point: PointId,
        /// Unit direction of the slide.
        dir: Vector2,
    },
    /// The radius of a circle whose radius is free, as an offset.
    Radius {
        /// Index of the sketch in `PartDocument::sketches`.
        sketch: usize,
        /// The circle.
        circle: EntityId,
    },
    /// The distance of a feature (Extrude), as an offset from its value.
    Distance {
        /// Index of the feature in `PartDocument::features`.
        feature: usize,
    },
}

/// Most unknowns one part brings (each costs a regeneration per step).
pub const MAX_UNKNOWNS: usize = 16;

/// The smallest distance an adaptive extrusion may come to.
const MIN_DISTANCE: Real = 0.01;

/// The unknowns of `part`: the free points of its adaptive sketches (also
/// of the sketches of its adaptive features) and the distances of its
/// adaptive extrusions with a Distance extent.
pub fn unknowns(part: &PartDocument) -> Vec<Unknown> {
    let mut sketches: Vec<usize> = Vec::new();
    let mut out = Vec::new();
    for (k, f) in part.features.iter().enumerate() {
        if !part.is_adaptive(f.id()) || part.is_suppressed(k) {
            continue;
        }
        if let Some(s) = f.sketch().and_then(|id| part.sketch_index(id))
            && !sketches.contains(&s)
        {
            sketches.push(s);
        }
        if let Feature::Extrude(e) = f
            && matches!(e.extent, crate::part::Extent::Distance)
        {
            out.push(Unknown::Distance { feature: k });
        }
    }
    for (i, ps) in part.sketches.iter().enumerate() {
        if part.is_adaptive(&ps.id) && !sketches.contains(&i) {
            sketches.push(i);
        }
    }
    sketches.sort_unstable();
    for i in sketches {
        let sk = &part.sketches[i];
        let dof = sk.sketch.dof_analysis();
        // In the order of the sketch's points (deterministic).
        for (p, _) in sk.sketch.points() {
            match dof.points.get(&p) {
                Some(PointFreedom::Along(d)) => out.push(Unknown::Point {
                    sketch: i,
                    point: p,
                    dir: *d,
                }),
                Some(PointFreedom::Free) => {
                    for dir in [Vector2::x(), Vector2::y()] {
                        out.push(Unknown::Point {
                            sketch: i,
                            point: p,
                            dir,
                        });
                    }
                }
                _ => {}
            }
        }
        for &c in &dof.free_radius {
            if matches!(sk.sketch.entity(c), Some(Entity::Circle { .. })) {
                out.push(Unknown::Radius {
                    sketch: i,
                    circle: c,
                });
            }
        }
    }
    out.truncate(MAX_UNKNOWNS);
    out
}

/// `base` with the unknowns moved by `x`, re-solved and regenerated; `Err`
/// when a sketch cannot be solved or a feature that was healthy fails.
pub fn apply(base: &PartDocument, u: &[Unknown], x: &[Real]) -> Result<PartDocument, String> {
    let mut part = base.clone();
    // Each point's offset is the sum of its unknowns' (x, y of a free one),
    // from the base's position (x is absolute).
    let mut offsets: Vec<(usize, PointId, Vector2)> = Vec::new();
    let mut touched: Vec<usize> = Vec::new();
    for (k, v) in u.iter().zip(x) {
        match k {
            Unknown::Point { sketch, point, dir } => {
                match offsets
                    .iter_mut()
                    .find(|(s, p, _)| s == sketch && p == point)
                {
                    Some(o) => o.2 += dir * *v,
                    None => offsets.push((*sketch, *point, dir * *v)),
                }
            }
            Unknown::Radius { sketch, circle } => {
                let Some(Entity::Circle { radius, .. }) =
                    base.sketches[*sketch].sketch.entity(*circle)
                else {
                    return Err("a circle no longer exists".into());
                };
                let r = radius + v;
                if r < MIN_DISTANCE {
                    return Err("the circle would vanish".into());
                }
                part.sketches[*sketch]
                    .sketch
                    .set_circle_radius(*circle, r)
                    .map_err(|e| e.to_string())?;
                if !touched.contains(sketch) {
                    touched.push(*sketch);
                }
            }
            Unknown::Distance { feature } => {
                let d0 = params::feature_value(&base.features[*feature], "distance")
                    .ok_or("the feature has no distance")?;
                let d = d0 + v;
                if d < MIN_DISTANCE {
                    return Err("the extrusion would vanish".into());
                }
                params::set_feature_field_value(&mut part.features[*feature], "distance", d);
            }
        }
    }
    for (sketch, point, off) in offsets {
        let from = base.sketches[sketch]
            .sketch
            .point(point)
            .ok_or("a sketch point no longer exists")?
            .position;
        part.sketches[sketch]
            .sketch
            .move_point(point, from + off)
            .map_err(|e| e.to_string())?;
        if !touched.contains(&sketch) {
            touched.push(sketch);
        }
    }
    for i in touched {
        let r = part.sketches[i].sketch.solve().map_err(|e| e.to_string())?;
        if r.status != crate::sketch::SolveStatus::Solved {
            return Err(format!("{} cannot be solved", part.sketches[i].name));
        }
    }
    part.regenerate().map_err(|e| e.to_string())?;
    if let Some((id, why)) = part
        .feature_health
        .iter()
        .find(|(id, _)| !base.feature_health.contains_key(*id))
    {
        return Err(format!("{id}: {why}"));
    }
    Ok(part)
}

/// What [`adapt`] found.
#[derive(Clone, Debug)]
pub struct Adapted {
    /// The part with its new size.
    pub part: PartDocument,
    /// The assembly solved with it.
    pub solved: Solved,
    /// The unknowns' final offsets (mm), in [`unknowns`] order.
    pub x: Vec<Real>,
}

/// Scale of the rotation unknowns (mm per radian), as the solver's.
const TURN: Real = 50.0;

/// `asm` with the components of `group` moved by `q` (six per component:
/// a translation and a rotation vector × [`TURN`] about its own origin).
fn moved(asm: &AssemblyDocument, group: &[u32], q: &[Real]) -> AssemblyDocument {
    let mut a = asm.clone();
    for (k, id) in group.iter().enumerate() {
        if let Some(o) = a.find_mut(*id) {
            let d = &q[6 * k..6 * k + 6];
            let p = o.transform.translation.vector;
            let r = Transform3::new(Vector3::zeros(), Vector3::new(d[3], d[4], d[5]) / TURN);
            o.transform = Transform3::translation(p.x + d[0], p.y + d[1], p.z + d[2])
                * r
                * Transform3::translation(-p.x, -p.y, -p.z)
                * o.transform;
        }
    }
    a
}

/// The assembly `asm` solved with occurrence `occurrence` adaptive: its
/// part `base` changes as little as needed for the relationships to hold.
/// `local` gives every other selection's geometry (in its component's
/// coordinates); selections on occurrences of the adaptive part are read
/// from the trial part. The part's sizes and the placements of the free
/// components joined to it are solved together (Levenberg, finite
/// differences: one regeneration per size and step), then the whole
/// assembly is solved rigidly with the new part. `Ok(None)` when the rigid
/// solve already holds or the part has no unknowns; `Err` when no size
/// meets the relationships (nothing is changed then, rule 5).
pub fn adapt(
    asm: &AssemblyDocument,
    occurrence: u32,
    base: &PartDocument,
    local: &dyn Fn(&GeomRef) -> Result<Geom, String>,
) -> Result<Option<Adapted>, String> {
    adapt_with(asm, &Target::top(occurrence), base, local, None)
}

/// Where the adaptive part is: an occurrence of the assembly, or a part
/// inside one of its subassemblies (path `sub` below occurrence `top`,
/// placed `at` in that occurrence's coordinates).
#[derive(Clone, Debug)]
pub struct Target {
    /// The occurrence of the assembly (the subassembly holding the part).
    pub top: u32,
    /// The path below it (empty: `top` is the part itself).
    pub sub: Vec<u32>,
    /// The part's placement in `top`'s coordinates.
    pub at: Transform3,
}

impl Target {
    /// The adaptive part is occurrence `occurrence` itself.
    pub fn top(occurrence: u32) -> Self {
        Self {
            top: occurrence,
            sub: Vec::new(),
            at: Transform3::identity(),
        }
    }
}

/// [`adapt`] of a part inside a subassembly (spec 3.10 §3.4): the
/// relationships of this assembly that reach it (by its path) read the trial
/// part, carried into the subassembly occurrence's coordinates; the free
/// components joined to the subassembly move with the sizes, the
/// subassembly itself stays rigid.
pub fn adapt_nested(
    asm: &AssemblyDocument,
    target: &Target,
    base: &PartDocument,
    local: &dyn Fn(&GeomRef) -> Result<Geom, String>,
    drag: Option<&solve::Drag>,
) -> Result<Option<Adapted>, String> {
    adapt_with(asm, target, base, local, drag)
}

/// [`adapt`] while `drag` pulls a component joined to the adaptive part
/// (Inventor stretches an adaptive part as its partner is dragged): the
/// drag's goal is one more, weak, residual of the joint solve (as in the
/// rigid drag), so the part takes the size that brings the grabbed point
/// nearest the cursor while every relationship holds. The rigid solve is
/// not asked first (with a drag it always "holds": the dragged component
/// stops where the rigid part lets it). `Ok(None)` when the dragged
/// component is not joined to the part or the size need not change.
pub fn adapt_dragged(
    asm: &AssemblyDocument,
    occurrence: u32,
    base: &PartDocument,
    local: &dyn Fn(&GeomRef) -> Result<Geom, String>,
    drag: &solve::Drag,
) -> Result<Option<Adapted>, String> {
    adapt_with(asm, &Target::top(occurrence), base, local, Some(drag))
}

fn adapt_with(
    asm: &AssemblyDocument,
    target: &Target,
    base: &PartDocument,
    local: &dyn Fn(&GeomRef) -> Result<Geom, String>,
    drag: Option<&solve::Drag>,
) -> Result<Option<Adapted>, String> {
    let occurrence = target.top;
    let comp = asm
        .find(occurrence)
        .ok_or("the adaptive component no longer exists")?
        .component;
    let nested = !target.sub.is_empty();
    let mine = |r: &GeomRef| {
        if matches!(r, GeomRef::Origin { .. }) || r.occurrence() == relation::ASSEMBLY {
            return false;
        }
        if nested {
            return r.occurrence() == occurrence && r.sub() == target.sub.as_slice();
        }
        r.sub().is_empty()
            && asm
                .find(r.occurrence())
                .is_some_and(|o| o.component == comp)
    };
    let look_in = |part: &PartDocument, r: &GeomRef| {
        if !mine(r) {
            local(r)
        } else if nested {
            relation::resolve_in(part, r).map(|g| g.moved(&target.at))
        } else {
            relation::resolve_in(part, r)
        }
    };
    if drag.is_none() && solve::solve(asm, &|r: &GeomRef| look_in(base, r), None).converged {
        return Ok(None);
    }
    let u = unknowns(base);
    if u.is_empty() {
        return Ok(None);
    }
    let k = u.len();
    let mut group = solve::free_group(asm, occurrence);
    // A grounded adaptive part still adapts (it only does not move).
    group.retain(|id| asm.find(*id).is_some_and(|o| !o.grounded));
    // The dragged component's place among the unknowns.
    let pulled = match drag {
        Some(d) => match group.iter().position(|id| *id == d.occurrence) {
            Some(g) => Some((g, d)),
            None => return Ok(None),
        },
        None => None,
    };
    let n = k + 6 * group.len();
    let (_, sick) = solve::residuals_at(asm, &|r: &GeomRef| look_in(base, r));
    // The residuals with sizes x (a trial part) and moves q.
    let res = |part: &PartDocument, q: &[Real]| -> Option<Vec<Real>> {
        let a = moved(asm, &group, q);
        let (mut r, s) = solve::residuals_at(&a, &|r: &GeomRef| look_in(part, r));
        if let Some((_, d)) = pulled {
            let o = a.find(d.occurrence)?;
            let w = o.transform * d.local - d.target;
            r.extend([w.x * solve::GOAL, w.y * solve::GOAL, w.z * solve::GOAL]);
        }
        (s == sick).then_some(r)
    };
    let cost = |r: &[Real]| r.iter().map(|v| v * v).sum::<Real>();
    let mut z = vec![0.0; n];
    let mut part = base.clone();
    let mut r0 = res(&part, &z[k..]).ok_or("the relationships lost their geometry")?;
    let mut c = cost(&r0);
    let mut lambda = 1e-3;
    const H: Real = 1e-5;
    for _ in 0..60 {
        if r0.iter().all(|v| v.abs() < 1e-9) {
            break;
        }
        // Forward differences: sizes regenerate, moves do not.
        let mut jac: Vec<Vec<Real>> = Vec::with_capacity(n);
        for j in 0..n {
            let mut zj = z.clone();
            zj[j] += H;
            let r = if j < k {
                apply(base, &u, &zj[..k])
                    .ok()
                    .and_then(|p| res(&p, &zj[k..]))
            } else {
                res(&part, &zj[k..])
            };
            jac.push(match r {
                Some(r) => r.iter().zip(&r0).map(|(a, b)| (a - b) / H).collect(),
                None => vec![0.0; r0.len()],
            });
        }
        // Minimum change: (JᵀJ + λ(I + diag JᵀJ)) δ = −Jᵀr.
        let jtj = nalgebra::DMatrix::from_fn(n, n, |a, b| {
            jac[a].iter().zip(&jac[b]).map(|(p, q)| p * q).sum::<Real>()
        });
        let jtr = nalgebra::DVector::from_fn(n, |a, _| {
            jac[a].iter().zip(&r0).map(|(p, q)| p * q).sum::<Real>()
        });
        if jtr.norm() < 1e-14 {
            break;
        }
        let mut stepped = false;
        let before = c;
        for _ in 0..12 {
            let mut m = jtj.clone();
            for a in 0..n {
                m[(a, a)] += lambda * (1.0 + jtj[(a, a)]);
            }
            let Some(d) = m.lu().solve(&(-&jtr)) else {
                lambda *= 10.0;
                continue;
            };
            let zn: Vec<Real> = z.iter().zip(d.iter()).map(|(a, b)| a + b).collect();
            let trial = if d.iter().take(k).any(|v| *v != 0.0) {
                apply(base, &u, &zn[..k]).ok()
            } else {
                Some(part.clone())
            };
            if let Some(p) = trial
                && let Some(r) = res(&p, &zn[k..])
                && cost(&r) < c
            {
                c = cost(&r);
                z = zn;
                part = p;
                r0 = r;
                lambda = (lambda / 4.0).max(1e-12);
                stepped = true;
                break;
            }
            lambda *= 8.0;
        }
        if !stepped {
            break;
        }
        // With a drag the goal is never met exactly: stop when a step
        // gains (almost) nothing.
        if pulled.is_some() && before - c < 1e-12 * before.max(1e-30) {
            break;
        }
    }
    // Sizes below 0.1 µm are the finite differences' noise.
    if pulled.is_some() && z[..k].iter().all(|v| v.abs() < 1e-4) {
        return Ok(None);
    }
    // The whole assembly with the new part, from the found placements.
    let start = moved(asm, &group, &z[k..]);
    let solved = solve::solve(&start, &|r: &GeomRef| look_in(&part, r), None);
    if !solved.converged {
        return Err(format!(
            "no size of the adaptive part meets the relationships ({:.3} mm left)",
            solved.residual
        ));
    }
    let mut solved = solved;
    // Components the joint step moved but the final solve did not.
    for id in &group {
        if !solved.transforms.iter().any(|(i, _)| i == id)
            && let Some(o) = start.find(*id)
        {
            solved.transforms.push((*id, o.transform));
        }
    }
    Ok(Some(Adapted {
        part,
        solved,
        x: z[..k].to_vec(),
    }))
}
