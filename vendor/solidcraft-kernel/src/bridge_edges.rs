//! Loft between the edges of surfaces (3DCadCraft addition, carried over from our truck version):
//! one bridge per pair of sides of two boundaries, each side a row of points on its edges, made
//! by [`crate::bridge`] and stitched into one surface body.
//!
//! A closed boundary is cut into sides at its corners. A smooth one (a circle) is cut where it
//! comes nearest the other boundary's corners, or into quarters when neither has corners. With
//! different numbers of corners the boundary with more keeps those nearest the other's and the
//! rest stay inside their bridge. The second boundary is run the way, and from the corner, that
//! pairs the corners nearest. Holes pair with the nearest hole of the other surface.

use solidcore_record::Vec3;
use solidcore_record::brep::Loop;
use solidcraft_geom::prelude::*;

use crate::body::Body;
use crate::bridge::{ChainLine, Continuity, Frame, bridge, frames_across};
use crate::surfaces::{Chain, edge_near, picked_chain, stitch};
use crate::{KernelError, Result};

/// Points per edge when a boundary is walked: rows must lie on the edges within the tolerance a
/// later Stitch uses.
const PER_EDGE: usize = 256;
/// Stations of one bridge.
const STATIONS: usize = 129;
/// A joint that turns more than this is a corner (smooth joints of split edges turn about 1°).
const CORNER_DEGREES: f64 = 10.0;

fn invalid(what: &str) -> KernelError {
    KernelError::Invalid(what.into())
}

/// A point of a boundary, the edge under it, and whether the walk runs along that edge.
#[derive(Clone, Copy)]
struct Stop {
    p: Vec3,
    edge: usize,
    along: bool,
}

/// A boundary walked once: its stops, and the stops where its edges join.
struct Walked {
    stops: Vec<Stop>,
    joints: Vec<usize>,
    closed: bool,
}

/// The picks divided into chains by the vertices their edges share.
fn chains_of(body: &Body, at: &[Vec3]) -> Result<Vec<(Chain, bool)>> {
    let r = body.record();
    let mut groups: Vec<(Vec<u32>, Vec<Vec3>)> = Vec::new();
    for p in at {
        let e = edge_near(body, *p).ok_or_else(|| invalid("no edge near a picked point"))?;
        let v = r.edges.get(e).ok_or_else(|| invalid("an edge reference"))?.vertices;
        let touching: Vec<usize> =
            (0..groups.len()).filter(|&g| groups.get(g).is_some_and(|(vs, _)| vs.contains(&v[0]) || vs.contains(&v[1]))).collect();
        let mut merged = (v.to_vec(), vec![*p]);
        for &g in touching.iter().rev() {
            let (vs, pts) = groups.remove(g);
            merged.0.extend(vs);
            merged.1.extend(pts);
        }
        groups.push(merged);
    }
    groups.iter().map(|(_, pts)| picked_chain(body, pts)).collect()
}

fn walk(body: &Body, chain: &Chain, closed: bool) -> Result<Walked> {
    let mut stops: Vec<Stop> = Vec::new();
    let mut joints = Vec::new();
    for &(edge, along) in chain {
        let line = ChainLine::of(body, &vec![(edge, along)])?;
        let pts = line.stations(PER_EDGE + 1, 0.0, false)?;
        // The edge before ended where this one starts.
        stops.pop();
        joints.push(stops.len());
        stops.extend(pts.iter().map(|(p, _)| Stop { p: *p, edge, along }));
    }
    if closed {
        // The walk ends where it began.
        stops.pop();
    }
    Ok(Walked { stops, joints, closed })
}

fn length(row: &[Stop]) -> f64 {
    row.windows(2).filter_map(|w| Some(w.first()?.p.dist(w.get(1)?.p))).sum()
}

fn centre(row: &[Stop]) -> Vec3 {
    let n = row.len().max(1) as f64;
    row.iter().fold(Vec3::ZERO, |s, x| s + x.p) * (1.0 / n)
}

impl Walked {
    fn stop(&self, i: usize) -> Option<Stop> {
        self.stops.get(i % self.stops.len().max(1)).copied()
    }

    /// The joints that turn sharply.
    fn corners(&self) -> Vec<usize> {
        let n = self.stops.len();
        let cos = CORNER_DEGREES.to_radians().cos();
        self.joints
            .iter()
            .copied()
            .filter(|&k| {
                let (Some(a), Some(b), Some(c)) = (self.stop(k + n - 1), self.stop(k), self.stop(k + 1)) else { return false };
                match ((b.p - a.p).normalized(), (c.p - b.p).normalized()) {
                    (Some(u), Some(v)) => u.dot(v) < cos,
                    _ => false,
                }
            })
            .collect()
    }

    /// The stop nearest a point.
    fn nearest(&self, p: Vec3) -> usize {
        let mut best = (f64::MAX, 0);
        for (i, s) in self.stops.iter().enumerate() {
            let d = s.p.dist(p);
            if d < best.0 {
                best = (d, i);
            }
        }
        best.1
    }

    /// The stops nearest `others` (points of the other boundary), each once, in walking order.
    fn nearest_to(&self, others: &[Vec3]) -> Vec<usize> {
        let mut cuts: Vec<usize> = others.iter().map(|p| self.nearest(*p)).collect();
        cuts.sort_unstable();
        cuts.dedup();
        cuts
    }

    /// Stops `n` even parts of the walk apart.
    fn evenly(&self, n: usize) -> Vec<usize> {
        (0..n).map(|k| k * self.stops.len() / n).collect()
    }

    /// The side from stop `a` to stop `b`, going round.
    fn side(&self, a: usize, b: usize) -> Vec<Stop> {
        let n = self.stops.len();
        let steps = if b > a { b - a } else { b + n - a };
        (0..=steps).filter_map(|k| self.stop(a + k)).collect()
    }

    fn sides(&self, cuts: &[usize]) -> Vec<Vec<Stop>> {
        let n = cuts.len();
        (0..n).filter_map(|k| Some(self.side(*cuts.get(k)?, *cuts.get((k + 1) % n)?))).collect()
    }
}

/// Where each boundary is cut, so that both have as many sides.
fn cuts_of(a: &Walked, b: &Walked) -> Result<(Vec<usize>, Vec<usize>)> {
    let points = |w: &Walked, cuts: &[usize]| -> Vec<Vec3> { cuts.iter().filter_map(|&k| Some(w.stop(k)?.p)).collect() };
    let (ca, cb) = (a.corners(), b.corners());
    let (ca, cb) = match (ca.is_empty(), cb.is_empty()) {
        (true, true) => {
            let first = if a.joints.len() >= 2 { a.joints.clone() } else { a.evenly(4) };
            let other = b.nearest_to(&points(a, &first));
            (first, other)
        }
        (true, false) => (a.nearest_to(&points(b, &cb)), cb),
        (false, true) => {
            let other = b.nearest_to(&points(a, &ca));
            (ca, other)
        }
        (false, false) if ca.len() > cb.len() => {
            let kept: Vec<usize> = {
                let wanted = points(b, &cb);
                let mut c: Vec<usize> = wanted
                    .iter()
                    .filter_map(|p| ca.iter().copied().min_by(|x, y| dist_to(a, *x, *p).total_cmp(&dist_to(a, *y, *p))))
                    .collect();
                c.sort_unstable();
                c.dedup();
                c
            };
            (kept, cb)
        }
        (false, false) if cb.len() > ca.len() => {
            let kept: Vec<usize> = {
                let wanted = points(a, &ca);
                let mut c: Vec<usize> = wanted
                    .iter()
                    .filter_map(|p| cb.iter().copied().min_by(|x, y| dist_to(b, *x, *p).total_cmp(&dist_to(b, *y, *p))))
                    .collect();
                c.sort_unstable();
                c.dedup();
                c
            };
            (ca, kept)
        }
        _ => (ca, cb),
    };
    if ca.len() != cb.len() || ca.len() < 2 {
        return Err(KernelError::Invalid(format!("the boundaries have {} and {} sides (corners) that could not be paired", ca.len(), cb.len())));
    }
    Ok((ca, cb))
}

fn dist_to(w: &Walked, k: usize, p: Vec3) -> f64 {
    w.stop(k).map_or(f64::MAX, |s| s.p.dist(p))
}

/// A row run the other way.
fn turned(row: &[Stop]) -> Vec<Stop> {
    row.iter().rev().map(|s| Stop { along: !s.along, ..*s }).collect()
}

/// `count` stops evenly along a row; its ends stay its ends.
fn resampled(row: &[Stop], count: usize) -> Vec<Stop> {
    let total = length(row);
    let mut out = Vec::with_capacity(count);
    let mut i = 0;
    let mut walked = 0.0;
    for k in 0..count {
        let target = total * k as f64 / (count - 1) as f64;
        while i + 2 < row.len() {
            let Some(step) = row.get(i).zip(row.get(i + 1)).map(|(a, b)| a.p.dist(b.p)) else { break };
            if walked + step >= target {
                break;
            }
            walked += step;
            i += 1;
        }
        let (Some(a), Some(b)) = (row.get(i), row.get(i + 1)) else { continue };
        let step = a.p.dist(b.p);
        let f = if step > 1e-12 { ((target - walked) / step).clamp(0.0, 1.0) } else { 0.0 };
        out.push(Stop { p: a.p + (b.p - a.p) * f, ..*a });
    }
    if let (Some(slot), Some(last)) = (out.last_mut(), row.last()) {
        slot.p = last.p;
    }
    out
}

/// The face using `edge` and whether its loop runs along the edge's own direction.
fn owner(body: &Body, edge: usize) -> Result<(usize, bool)> {
    let r = body.record();
    let use_ = r.edges.get(edge).and_then(|e| e.uses.first()).ok_or_else(|| invalid("an edge of a section bounds no face"))?;
    let forward = r
        .faces
        .get(use_.face as usize)
        .and_then(|f| f.loops.get(use_.r#loop as usize))
        .and_then(|l| match l {
            Loop::Edges { edges } => edges.get(use_.position as usize).map(|u| u.forward),
            Loop::Vertex { .. } => None,
        })
        .ok_or_else(|| invalid("an edge use"))?;
    Ok((use_.face as usize, forward))
}

/// Directions leaving `row`'s surface straight across its boundary, away from the surface.
fn leaving(body: &Body, row: &[Stop]) -> Result<Vec<Vec3>> {
    let size = length(row).max(1e-6);
    let mut out = Vec::with_capacity(row.len());
    for (i, s) in row.iter().enumerate() {
        let (a, b) = (row.get(i.saturating_sub(1)), row.get(i + 1));
        let run = match (a, b) {
            (Some(a), Some(b)) => b.p - a.p,
            (Some(a), None) => s.p - a.p,
            (None, Some(b)) => b.p - s.p,
            (None, None) => return Err(invalid("an empty side")),
        };
        let run = run.normalized().ok_or_else(|| invalid("a side has no direction"))?;
        let (face, forward) = owner(body, s.edge)?;
        let tangent = if s.along == forward { run } else { run * -1.0 };
        let got = frames_across(body, face, &[(s.p, tangent)], size, false)?;
        let (frame, _) = got.first().ok_or_else(|| invalid("a station without a frame"))?;
        out.push(frame.leave);
    }
    Ok(out)
}

struct SidePair {
    a: Vec<Stop>,
    b: Vec<Stop>,
    leave_a: Option<Vec<Vec3>>,
    leave_b: Option<Vec<Vec3>>,
}

/// Faces of the pairs, the corner directions of neighbours averaged so that they share their
/// corner curves. The middle of a side keeps its own direction: the average fades in near the ends.
fn average_corners(pairs: &mut [SidePair]) {
    let n = pairs.len();
    if n < 2 {
        return;
    }
    fn fade(row: &mut [Vec3], to: Vec3, at_end: bool) {
        let n = row.len();
        let reach = ((n as f64 * 0.15).ceil() as usize).max(1);
        let end = if at_end { n.saturating_sub(1) } else { 0 };
        let Some(from) = row.get(end).copied() else { return };
        let delta = to - from;
        for s in 0..reach.min(n) {
            let i = if at_end { n - 1 - s } else { s };
            let x = s as f64 / reach as f64;
            if let Some(slot) = row.get_mut(i) {
                let moved = *slot + delta * (1.0 - x).powi(3);
                *slot = moved.normalized().unwrap_or(*slot);
            }
        }
    }
    for k in 0..n {
        let j = (k + 1) % n;
        for use_a in [true, false] {
            let pick = |p: &SidePair, at_end: bool| -> Option<Vec3> {
                let row = if use_a { p.leave_a.as_ref() } else { p.leave_b.as_ref() }?;
                if at_end { row.last().copied() } else { row.first().copied() }
            };
            let (Some(x), Some(y)) = (pairs.get(k).and_then(|p| pick(p, true)), pairs.get(j).and_then(|p| pick(p, false))) else { continue };
            let Some(mid) = (x + y).normalized() else { continue };
            for (i, at_end) in [(k, true), (j, false)] {
                if let Some(p) = pairs.get_mut(i) {
                    let row = if use_a { p.leave_a.as_mut() } else { p.leave_b.as_mut() };
                    if let Some(row) = row {
                        fade(row, mid, at_end);
                    }
                }
            }
        }
    }
}

fn zero_shape(p: Vec3, leave: Vec3) -> Frame {
    Frame { p, leave, bend: Vec3::ZERO, bend_change: Vec3::ZERO }
}

/// How far a `Some(weight)` reaches, as the bridge's crown. The bridge's speed there is
/// `(1 + crown)` times a third of the distance across, and a cubic's second control row lies a
/// third of the speed away: `w` times a third of the distance is `1 + crown = 3 w`.
fn crown(weight: Option<f64>) -> f64 {
    weight.map_or(0.0, |w| (3.0 * w - 1.0).clamp(-0.9, 3.0))
}

fn bridge_pair(pair: &SidePair, weights: [Option<f64>; 2]) -> Result<Body> {
    let across = |row: &[Stop], other: &[Stop], leave: &Option<Vec<Vec3>>| -> Result<Vec<Frame>> {
        row.iter()
            .enumerate()
            .map(|(i, s)| {
                let dir = match leave.as_ref().and_then(|l| l.get(i)) {
                    Some(l) => *l,
                    None => {
                        let q = other.get(i).ok_or_else(|| invalid("rows of different lengths"))?;
                        (q.p - s.p).normalized().ok_or_else(|| invalid("the sections touch: use Boundary Patch"))?
                    }
                };
                Ok(zero_shape(s.p, dir))
            })
            .collect()
    };
    let (fa, fb) = (across(&pair.a, &pair.b, &pair.leave_a)?, across(&pair.b, &pair.a, &pair.leave_b)?);
    let order = |w: Option<f64>| if w.is_some() { Continuity::G1 } else { Continuity::G0 };
    bridge(&fa, &fb, [order(weights[0]), order(weights[1])], [crown(weights[0]), crown(weights[1])])
}

fn pairs_of_loops(a: &Body, la: &Walked, b: &Body, lb: &Walked, weights: [Option<f64>; 2]) -> Result<Vec<SidePair>> {
    let (ca, cb) = cuts_of(la, lb)?;
    let (sa, sb) = (la.sides(&ca), lb.sides(&cb));
    let n = sa.len();
    let start = |side: &Vec<Stop>| side.first().map(|s| s.p);
    let back: Vec<Vec<Stop>> = sb.iter().rev().map(|s| turned(s)).collect();
    let mut best: Option<(f64, Vec<Vec<Stop>>)> = None;
    for list in [&sb, &back] {
        for r in 0..n {
            let cand: Vec<Vec<Stop>> = (0..n).filter_map(|k| list.get((k + r) % n).cloned()).collect();
            let cost: f64 = sa
                .iter()
                .zip(&cand)
                .map(|(x, y)| match (start(x), start(y)) {
                    (Some(p), Some(q)) => p.dist(q),
                    _ => f64::MAX,
                })
                .sum();
            if best.as_ref().is_none_or(|(c, _)| cost < *c) {
                best = Some((cost, cand));
            }
        }
    }
    let pb = best.map(|(_, c)| c).ok_or_else(|| invalid("no pairing of the sides"))?;
    let mut pairs = Vec::with_capacity(n);
    for (x, y) in sa.iter().zip(&pb) {
        pairs.push(make_pair(a, x, b, y, weights)?);
    }
    average_corners(&mut pairs);
    Ok(pairs)
}

fn make_pair(a: &Body, x: &[Stop], b: &Body, y: &[Stop], weights: [Option<f64>; 2]) -> Result<SidePair> {
    let (ra, rb) = (resampled(x, STATIONS), resampled(y, STATIONS));
    let leave_a = if weights[0].is_some() { Some(leaving(a, &ra)?) } else { None };
    let leave_b = if weights[1].is_some() { Some(leaving(b, &rb)?) } else { None };
    Ok(SidePair { a: ra, b: rb, leave_a, leave_b })
}

/// [`crate::bridge`] between the edges of two surfaces, each edge picked by a point on it:
/// G0 at a section whose `weights` entry is `None`, tangent to its surface otherwise, reaching on
/// `w` times a third of the distance across. Closed boundaries give one face per pair of sides,
/// holes paired too; open chains one face. One surface body.
pub fn bridge_edges_tangent(a: &Body, at_a: &[Vec3], b: &Body, at_b: &[Vec3], weights: [Option<f64>; 2]) -> Result<Body> {
    a.require_brep("bridge")?;
    b.require_brep("bridge")?;
    if at_a.is_empty() || at_b.is_empty() || at_a.len() + at_b.len() > 10_000 {
        return Err(invalid("pick the edges of both sections"));
    }
    let walks = |body: &Body, at: &[Vec3]| -> Result<Vec<Walked>> {
        let mut out: Vec<Walked> = chains_of(body, at)?.iter().map(|(c, closed)| walk(body, c, *closed)).collect::<Result<_>>()?;
        out.sort_by(|x, y| length(&y.stops).total_cmp(&length(&x.stops)));
        Ok(out)
    };
    let (wa, wb) = (walks(a, at_a)?, walks(b, at_b)?);
    let mut pairs = Vec::new();
    if wa.iter().chain(&wb).all(|w| w.closed) {
        let (Some(oa), Some(ob)) = (wa.first(), wb.first()) else { return Err(invalid("pick the edges of both sections")) };
        pairs.extend(pairs_of_loops(a, oa, b, ob, weights)?);
        // The holes: as many pairs as the fewer has, each with the nearest of the other.
        let mut free: Vec<&Walked> = wb.iter().skip(1).collect();
        for h in wa.iter().skip(1) {
            let c = centre(&h.stops);
            let Some(k) = (0..free.len()).min_by(|x, y| {
                let d = |i: &usize| free.get(*i).map_or(f64::MAX, |w| centre(&w.stops).dist(c));
                d(x).total_cmp(&d(y))
            }) else {
                break;
            };
            let other = free.remove(k);
            pairs.extend(pairs_of_loops(a, h, b, other, weights)?);
        }
    } else {
        let ([one_a], [one_b]) = (&wa[..], &wb[..]) else { return Err(invalid("pick one chain of edges on each section")) };
        if one_a.closed || one_b.closed {
            return Err(invalid("a closed boundary cannot be bridged to an open chain"));
        }
        let (mut ya, xa) = (one_b.stops.clone(), &one_a.stops);
        let ends = |r: &[Stop]| Some((r.first()?.p, r.last()?.p));
        let (Some((a0, a1)), Some((b0, b1))) = (ends(xa), ends(&ya)) else { return Err(invalid("a section has no edges")) };
        if a0.dist(b1) + a1.dist(b0) < a0.dist(b0) + a1.dist(b1) {
            ya = turned(&ya);
        }
        pairs.push(make_pair(a, xa, b, &ya, weights)?);
    }
    let bodies: Vec<Body> = pairs.iter().map(|p| bridge_pair(p, weights)).collect::<Result<_>>()?;
    match &bodies[..] {
        [] => Err(invalid("nothing to bridge")),
        [one] => Ok(one.clone()),
        many => {
            let refs: Vec<&Body> = many.iter().collect();
            stitch(&refs, 0.0)
        }
    }
}
