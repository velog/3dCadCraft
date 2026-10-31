//! Lofted Flange (spec 5.6 §6.1k): a transition between two profiles (each a
//! polyline of points, open or closed). Both are split at the same relative
//! lengths (every vertex of either gives a station on both), the strip
//! between them is triangulated and thickened into a solid of triangles; the
//! flat pattern is the strip developed triangle by triangle on its middle
//! surface (lengths kept), opened at the seam of closed profiles. Press Brake
//! output keeps the facets' edges as bend lines of the flat.

use std::sync::Arc;

use solidcraft_doc::sheet as sd;
use solidcraft_geom::{Loop2, Plane as SPlane, Region2, Vec2, Vec3};
use solidcraft_kernel::history::FaceOrigin;

use super::{BendLine, ExactSolid, RuleValues, SheetModel, rule_of};
use crate::kernel::{Point3, Real, Vector3};

/// A profile of a Lofted Flange: points along it (lines kept, arcs already
/// split into facets or fine chords), closed when it is a loop.
#[derive(Clone, Debug)]
pub struct LoftProfile {
    /// The points in order (a closed profile does not repeat its first).
    pub points: Vec<Point3>,
    /// A loop.
    pub closed: bool,
}

/// A Lofted Flange's result.
pub struct Lofted {
    /// The solid (triangles).
    pub solid: ExactSolid,
    /// The sheet model whose flat pattern is the developed strip.
    pub model: SheetModel,
    /// Bend lines of the flat (Press Brake: the facets' edges).
    pub bends: Vec<BendLine>,
    /// The middle surface's area (mm²): the flat's.
    pub area: Real,
}

/// Relative lengths along a polyline (0 at the start, 1 at the end; a
/// closed one runs back to its first point).
fn params(p: &[Point3], closed: bool) -> Vec<Real> {
    let n = p.len();
    let m = if closed { n + 1 } else { n };
    let mut t = Vec::with_capacity(m);
    let mut s = 0.0;
    t.push(0.0);
    for k in 1..m {
        s += (p[k % n] - p[k - 1]).norm();
        t.push(s);
    }
    let total = s.max(1e-12);
    t.iter().map(|x| x / total).collect()
}

/// The point at relative length `u` of a polyline whose stations are `t`.
fn at(p: &[Point3], t: &[Real], u: Real) -> Point3 {
    let n = p.len();
    let k = t.partition_point(|x| *x <= u).clamp(1, t.len() - 1);
    let (t0, t1) = (t[k - 1], t[k]);
    let w = if t1 - t0 > 1e-15 {
        (u - t0) / (t1 - t0)
    } else {
        0.0
    };
    let (a, b) = (p[(k - 1) % n], p[k % n]);
    a + (b - a) * w
}

fn v3(p: &Point3) -> Vec3 {
    Vec3::new(p.x, p.y, p.z)
}

/// `b` turned to run like `a`: same direction, a closed loop started at its
/// point nearest `a`'s start.
fn align(a: &LoftProfile, b: &LoftProfile) -> Vec<Point3> {
    let mut best = b.points.clone();
    let mut cost = Real::MAX;
    let samples = [0.0, 0.25, 0.5, 0.75, 1.0];
    let ta = params(&a.points, a.closed);
    let mut try_one = |cand: Vec<Point3>| {
        let tc = params(&cand, b.closed);
        let c: Real = samples
            .iter()
            .map(|u| (at(&a.points, &ta, *u) - at(&cand, &tc, *u)).norm())
            .sum();
        if c < cost {
            cost = c;
            best = cand;
        }
    };
    for rev in [false, true] {
        let mut q = b.points.clone();
        if rev {
            q.reverse();
        }
        if b.closed {
            let start = a.points[0];
            let k = q
                .iter()
                .enumerate()
                .min_by(|x, y| (x.1 - start).norm().total_cmp(&(y.1 - start).norm()))
                .map_or(0, |(k, _)| k);
            q.rotate_left(k);
        }
        try_one(q);
    }
    best
}

/// Lofts `a` to `b`. `alpha`: where the profiles lie in the thickness (0:
/// the thickness on the strip's normal side, 1: the other, 0.5: centred).
pub fn lofted_flange(
    prefix: &str,
    rv: &RuleValues,
    a: &LoftProfile,
    b: &LoftProfile,
    alpha: Real,
    press_brake: bool,
) -> Result<Lofted, String> {
    if a.closed != b.closed {
        return Err("the two profiles must both be open or both closed".into());
    }
    if a.points.len() < 2 || b.points.len() < 2 {
        return Err("a profile has no length".into());
    }
    let closed = a.closed;
    let bq = align(a, b);
    let (ta, tb) = (params(&a.points, closed), params(&bq, closed));
    // Stations: every vertex of either profile.
    let mut ts: Vec<Real> = ta.iter().chain(tb.iter()).copied().collect();
    ts.sort_by(|x, y| x.total_cmp(y));
    ts.dedup_by(|x, y| (*x - *y).abs() < 1e-9);
    let rows: Vec<(Point3, Point3)> = ts
        .iter()
        .map(|u| (at(&a.points, &ta, *u), at(&bq, &tb, *u)))
        .collect();
    let m = rows.len();
    if m < 2 {
        return Err("the profiles have no length".into());
    }
    let t = rv.thickness;
    // Columns across the strip: Die Form splits each generator into pieces of
    // about 5 mm (the vertex normals then follow the surface's twist, so the
    // thickness stays square to it); Press Brake keeps its facets whole.
    let cols = if press_brake {
        1
    } else {
        let longest = rows
            .iter()
            .map(|(p, q)| (q - p).norm())
            .fold(0.0, Real::max);
        ((longest / 5.0).ceil() as usize).clamp(1, 16)
    };
    let grid = |i: usize, c: usize| {
        let (p, q) = rows[i];
        p + (q - p) * (c as Real / cols as Real)
    };
    // Each cell's two triangles, split along the shorter diagonal, as (row, column).
    let cell = |i: usize, c: usize| -> [[(usize, usize); 3]; 2] {
        let (a0, a1, b0, b1) = (
            grid(i, c),
            grid(i + 1, c),
            grid(i, c + 1),
            grid(i + 1, c + 1),
        );
        if (b1 - a0).norm() <= (b0 - a1).norm() {
            [
                [(i, c), (i + 1, c), (i + 1, c + 1)],
                [(i, c), (i + 1, c + 1), (i, c + 1)],
            ]
        } else {
            [
                [(i, c), (i + 1, c), (i, c + 1)],
                [(i + 1, c), (i + 1, c + 1), (i, c + 1)],
            ]
        }
    };
    let pos = |(r, c): (usize, usize)| grid(r, c);
    let quads = m - 1;
    let tris: Vec<[(usize, usize); 3]> = (0..quads)
        .flat_map(|i| (0..cols).flat_map(move |c| cell(i, c)))
        .collect();
    let w = cols + 1;
    // Vertex normals (area weighted); a closed strip's seam rows share them.
    let mut normals = vec![Vector3::zeros(); m * w];
    for tr in &tris {
        let (p, q, r) = (pos(tr[0]), pos(tr[1]), pos(tr[2]));
        let nrm = (q - p).cross(&(r - p));
        for &(i, c) in tr {
            normals[i * w + c] += nrm;
        }
    }
    if closed {
        for c in 0..w {
            let s = normals[c] + normals[(m - 1) * w + c];
            normals[c] = s;
            normals[(m - 1) * w + c] = s;
        }
    }
    let mut lo = vec![Point3::origin(); m * w];
    let mut hi = vec![Point3::origin(); m * w];
    let mut mid = vec![Point3::origin(); m * w];
    for i in 0..m {
        for c in 0..w {
            let n = normals[i * w + c]
                .try_normalize(1e-12)
                .ok_or("the profiles meet: no surface between them")?;
            let p = grid(i, c);
            lo[i * w + c] = p - n * (t * alpha);
            hi[i * w + c] = p + n * (t * (1.0 - alpha));
            mid[i * w + c] = p + n * (t * (0.5 - alpha));
        }
    }
    // The closed solid: hi surface, lo surface reversed, the two profile
    // walls and (open) the two end walls.
    let idx = |layer: usize, (i, c): (usize, usize)| {
        let i = if closed && i == m - 1 { 0 } else { i };
        (layer * m * w + i * w + c) as u32
    };
    let mut positions = Vec::with_capacity(m * w * 2);
    for layer in [&lo, &hi] {
        for p in layer.iter() {
            positions.push(v3(p));
        }
    }
    let mut tri_idx: Vec<[u32; 3]> = Vec::new();
    // Each triangle's face name (after the prefix).
    let mut groups: Vec<&str> = Vec::new();
    for tr in &tris {
        tri_idx.push([idx(1, tr[0]), idx(1, tr[1]), idx(1, tr[2])]);
        tri_idx.push([idx(0, tr[0]), idx(0, tr[2]), idx(0, tr[1])]);
        groups.extend(["Top", "Bottom"]);
    }
    for i in 0..quads {
        // Along the first profile (column 0): hi i+1 → i, down, back.
        let (ah0, ah1, al0, al1) = (
            idx(1, (i, 0)),
            idx(1, (i + 1, 0)),
            idx(0, (i, 0)),
            idx(0, (i + 1, 0)),
        );
        tri_idx.push([ah1, ah0, al0]);
        tri_idx.push([ah1, al0, al1]);
        groups.extend(["Edge1", "Edge1"]);
        // Along the second (the last column).
        let (bh0, bh1, bl0, bl1) = (
            idx(1, (i, cols)),
            idx(1, (i + 1, cols)),
            idx(0, (i, cols)),
            idx(0, (i + 1, cols)),
        );
        tri_idx.push([bh0, bh1, bl1]);
        tri_idx.push([bh0, bl1, bl0]);
        groups.extend(["Edge2", "Edge2"]);
    }
    if !closed {
        let e = m - 1;
        for c in 0..cols {
            let (a0h, b0h, a0l, b0l) = (
                idx(1, (0, c)),
                idx(1, (0, c + 1)),
                idx(0, (0, c)),
                idx(0, (0, c + 1)),
            );
            tri_idx.push([a0h, b0h, b0l]);
            tri_idx.push([a0h, b0l, a0l]);
            groups.extend(["Start", "Start"]);
            let (amh, bmh, aml, bml) = (
                idx(1, (e, c)),
                idx(1, (e, c + 1)),
                idx(0, (e, c)),
                idx(0, (e, c + 1)),
            );
            tri_idx.push([bmh, amh, aml]);
            tri_idx.push([bmh, aml, bml]);
            groups.extend(["End", "End"]);
        }
    }
    let body = super::super::guarded("lofted flange", || {
        solidcraft_kernel::polyhedron_body(&positions, &tri_idx)
    })?;
    let faces = body.face_count();
    let mut face_names: Vec<Option<String>> = match body.history() {
        Some(h) => h
            .faces
            .iter()
            .map(|fp| {
                fp.origins.iter().find_map(|o| match o {
                    FaceOrigin::Face { operand: 0, face } => {
                        // Die Form: the facets of each side are one smooth face.
                        groups.get(*face).map(|g| {
                            if press_brake {
                                format!("{prefix}:{g}")
                            } else {
                                format!("{prefix}:Smooth:{g}")
                            }
                        })
                    }
                    _ => None,
                })
            })
            .collect(),
        None => Vec::new(),
    };
    face_names.resize(faces, None);
    let solid = ExactSolid {
        sheet: false,
        name: Some(prefix.to_owned()),
        body,
        face_names,
        profiles: Vec::new(),
    };
    // The flat: the middle surface developed triangle by triangle, over the
    // strip's own triangles (one across each generator: a twisted strip has
    // no flat with inner points kept).
    let midp = |(i, c): (usize, usize)| mid[i * w + c];
    let coarse: Vec<[(usize, usize); 3]> = (0..quads)
        .flat_map(|i| {
            let (a0, a1, b0, b1) = (grid(i, 0), grid(i + 1, 0), grid(i, cols), grid(i + 1, cols));
            if (b1 - a0).norm() <= (b0 - a1).norm() {
                [
                    [(i, 0), (i + 1, 0), (i + 1, cols)],
                    [(i, 0), (i + 1, cols), (i, cols)],
                ]
            } else {
                [
                    [(i, 0), (i + 1, 0), (i, cols)],
                    [(i + 1, 0), (i + 1, cols), (i, cols)],
                ]
            }
        })
        .collect();
    let mut flat: Vec<Option<Vec2>> = vec![None; m * w];
    let (p0, q0) = (midp((0, 0)), midp((0, cols)));
    flat[0] = Some(Vec2::new(0.0, 0.0));
    flat[cols] = Some(Vec2::new((q0 - p0).norm(), 0.0));
    let area: Real = coarse
        .iter()
        .map(|tr| {
            let (p, q, r) = (midp(tr[0]), midp(tr[1]), midp(tr[2]));
            (q - p).cross(&(r - p)).norm() * 0.5
        })
        .sum();
    // Every triangle keeps the first one's orientation; a triangle is placed
    // once two of its corners are (passes until none is left).
    let mut sense = 0.0;
    let mut left: Vec<&[(usize, usize); 3]> = coarse.iter().collect();
    while !left.is_empty() {
        let before = left.len();
        let mut rest = Vec::new();
        for tr in left {
            let at_ = |k: usize| tr[k].0 * w + tr[k].1;
            let known: Vec<usize> = (0..3).filter(|&k| flat[at_(k)].is_some()).collect();
            if known.len() == 3 {
                continue;
            }
            if known.len() != 2 {
                rest.push(tr);
                continue;
            }
            let new = (0..3).find(|k| !known.contains(k)).unwrap_or(0);
            let (k1, k2) = (known[0], known[1]);
            let f = |k: usize| flat[at_(k)].unwrap_or(Vec2::new(0.0, 0.0));
            let (f1, f2) = (f(k1), f(k2));
            let (d1, d2) = (
                (midp(tr[new]) - midp(tr[k1])).norm(),
                (midp(tr[new]) - midp(tr[k2])).norm(),
            );
            let base = f2 - f1;
            let l = base.len().max(1e-12);
            let x = (d1 * d1 - d2 * d2 + l * l) / (2.0 * l);
            let h = (d1 * d1 - x * x).max(0.0).sqrt();
            let ex = base * (1.0 / l);
            let ey = Vec2::new(-ex.y, ex.x);
            let signed = |c: Vec2| {
                let mut v = [Vec2::new(0.0, 0.0); 3];
                v[k1] = f1;
                v[k2] = f2;
                v[new] = c;
                (v[1] - v[0]).cross(v[2] - v[0])
            };
            let mut cand = f1 + ex * x + ey * h;
            if sense == 0.0 {
                sense = signed(cand).signum();
            } else if signed(cand) * sense < 0.0 {
                cand = f1 + ex * x - ey * h;
            }
            flat[at_(new)] = Some(cand);
        }
        if rest.len() == before {
            return Err("the strip could not be developed".into());
        }
        left = rest;
    }
    let get = |i: usize, c: usize| flat[i * w + c].ok_or("the strip could not be developed");
    let mut outline = Vec::with_capacity(2 * (m + w));
    for i in 0..m {
        outline.push(get(i, 0)?);
    }
    for i in (0..m).rev() {
        outline.push(get(i, cols)?);
    }
    outline.dedup_by(|x, y| x.dist(*y) < 1e-7);
    let region = Region2 {
        outer: Loop2::polygon(&outline).ccw(),
        holes: Vec::new(),
    };
    let frame = SPlane {
        origin: Vec3::new(0.0, 0.0, 0.0),
        x: Vec3::new(1.0, 0.0, 0.0),
        y: Vec3::new(0.0, 1.0, 0.0),
    };
    let sheet = sd::base_flange(prefix, "rule", &rule_of(rv), frame, region);
    // Press Brake: a row edge between facets turning more than half a degree is a bend.
    let mut bends = Vec::new();
    if press_brake {
        for i in 1..m - 1 {
            let (before, after) = (cell(i - 1, 0), cell(i, 0));
            let nrm = |tr: [(usize, usize); 3]| {
                let (p, q, r) = (pos(tr[0]), pos(tr[1]), pos(tr[2]));
                (q - p).cross(&(r - p)).try_normalize(1e-12)
            };
            let (Some(n0), Some(n1)) = (nrm(before[1]), nrm(after[0])) else {
                continue;
            };
            let angle = n0.dot(&n1).clamp(-1.0, 1.0).acos();
            if angle.to_degrees() < 0.5 {
                continue;
            }
            let (s, e) = (get(i, 0)?, get(i, 1)?);
            bends.push(BendLine {
                angle_deg: angle.to_degrees(),
                radius: rv.bend_radius,
                up: n0.cross(&n1).dot(&(pos((i, 1)) - pos((i, 0)))) > 0.0,
                allowance: 0.0,
                start: [s.x, s.y],
                end: [e.x, e.y],
            });
        }
    }
    Ok(Lofted {
        solid,
        model: SheetModel(Arc::new(sheet)),
        bends,
        area,
    })
}
