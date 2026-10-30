//! Convert to Sheet Metal of a body that is already sheet metal (an imported
//! part, spec 5.6 §6.1e): from the fixed face the sheet's panels are walked
//! across its round bends — a cylinder whose axis runs along the edge it
//! meets — and the sheet model is built from them: the fixed face is the
//! base, every further panel a flange with the bend's own radius and angle,
//! its outline and holes taken over (Inventor detects the thickness and the
//! bends the same way). The model is folded again and must give the body's
//! volume; else the body is no sheet of constant thickness, or has features
//! the walk does not know yet.

use std::collections::HashSet;
use std::sync::Arc;

use solidcraft_doc::sheet as sd;
use solidcraft_geom::{Loop2, Plane as SPlane, Region2, Seg2, Vec2, Vec3};

use super::{ExactSolid, RuleValues, SheetModel, named_solid, rule_of};
use crate::kernel::{BRep, Curve, EdgeId, FaceId, Loop, Point3, Real, Sense, Surface, Vector3};

/// Most panels one walk takes (a loop of bends never ends otherwise).
const MAX_PANELS: usize = 500;

fn s3(p: Point3) -> Vec3 {
    Vec3::new(p.x, p.y, p.z)
}

fn d3(v: Vector3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

fn snapped(v: Vec2) -> Vec2 {
    let g = |x: f64| (x * 1e9).round() / 1e9;
    Vec2::new(g(v.x), g(v.y))
}

/// A planar face's outward normal and a point on it.
fn plane_of(b: &BRep, f: FaceId) -> Option<(Vec3, Vec3)> {
    let face = b.face(f)?;
    let Surface::Plane(pl) = b.surface(face.surface)? else {
        return None;
    };
    let n = d3(pl.normal.into_inner());
    let n = if face.sense == Sense::Same {
        n
    } else {
        n * -1.0
    };
    Some((n, s3(pl.origin)))
}

/// The point of `v`.
fn at(b: &BRep, v: crate::kernel::VertexId) -> Option<Vec3> {
    b.vertex(v).map(|x| s3(x.point))
}

/// A face loop in the 2D coordinates of `pl` (lines and arcs).
fn loop2(b: &BRep, lp: &Loop, pl: &SPlane) -> Result<Loop2, String> {
    let n = pl.normal();
    let to2 = |q: Vec3| {
        let d = q - pl.origin;
        Vec2::new(d.dot(pl.x), d.dot(pl.y))
    };
    let mut segs = Vec::new();
    for ce in &lp.coedges {
        let e = b.edge(ce.edge).ok_or("edge")?;
        let (s, t) = (
            at(b, e.start).ok_or("vertex")?,
            at(b, e.end).ok_or("vertex")?,
        );
        let (s, t) = if ce.sense == Sense::Same {
            (s, t)
        } else {
            (t, s)
        };
        match b.curve(e.curve).ok_or("curve")? {
            Curve::Line(_) => segs.push(Seg2::Line {
                a: to2(s),
                b: to2(t),
            }),
            Curve::Circle(c) => {
                let centre = to2(s3(c.center));
                let start = (to2(s) - centre).angle();
                let turn = e.range.max - e.range.min;
                let ccw = d3(c.normal.into_inner()).dot(n) > 0.0;
                let sweep = if ccw == (ce.sense == Sense::Same) {
                    turn
                } else {
                    -turn
                };
                segs.push(Seg2::Arc {
                    center: centre,
                    radius: c.radius,
                    start,
                    sweep,
                });
            }
            Curve::BSpline(_) => {
                return Err("not yet: free-form edges on the sheet's faces".into());
            }
        }
    }
    Ok(Loop2 { segs })
}

/// A plane frame on face `f` (outward normal `n`, a point `o` on it).
fn frame_on(n: Vec3, o: Vec3) -> SPlane {
    let x = n.any_perp().normalized().unwrap_or(Vec3::X);
    SPlane {
        origin: o,
        x,
        y: n.cross(x),
    }
}

/// The face's region (outer loop and holes) in `pl`.
fn region_of(b: &BRep, f: FaceId, pl: &SPlane) -> Result<Region2, String> {
    let face = b.face(f).ok_or("face")?;
    let outer = loop2(b, &face.outer, pl)?.ccw();
    let mut holes = Vec::new();
    for h in &face.inner {
        holes.push(loop2(b, h, pl)?.ccw().reversed());
    }
    Ok(Region2 { outer, holes })
}

/// A straight edge's ends.
fn line_ends(b: &BRep, e: EdgeId) -> Option<(Vec3, Vec3)> {
    let ed = b.edge(e)?;
    matches!(b.curve(ed.curve)?, Curve::Line(_)).then_some(())?;
    Some((at(b, ed.start)?, at(b, ed.end)?))
}

/// The face across edge `e` from `f`.
fn across(b: &BRep, f: FaceId, e: EdgeId) -> Option<FaceId> {
    b.edge_faces(e).into_iter().find(|&g| g != f)
}

/// The straight edges of a face's outer loop.
fn straight_edges(b: &BRep, f: FaceId) -> Vec<EdgeId> {
    b.face(f)
        .map(|face| {
            face.outer
                .coedges
                .iter()
                .map(|c| c.edge)
                .filter(|&e| line_ends(b, e).is_some())
                .collect()
        })
        .unwrap_or_default()
}

/// A bend found from a panel's edge: the cylinder pieces crossed, the face
/// on the far side (the next panel's face on the same skin), the radius of
/// the cylinder on this skin, its axis direction and a point on it.
struct Crossing {
    pieces: Vec<FaceId>,
    next: FaceId,
    /// The straight edge the bend meets `next` along.
    entry: EdgeId,
    radius: Real,
    axis_o: Vec3,
    axis: Vec3,
}

/// From panel face `f` across its straight edge `e`: a round bend (one or
/// more cylinder pieces of one axis running along `e`) to the next planar
/// face, or `None` (a rim face, a hole wall, anything else).
fn crossing(b: &BRep, f: FaceId, e: EdgeId, removed: &[(Vec3, Vec3)]) -> Option<Crossing> {
    let (ea, eb) = line_ends(b, e)?;
    let dir = (eb - ea).normalized()?;
    let mut cur = across(b, f, e)?;
    let mut via = e;
    let mut pieces = Vec::new();
    let mut cyl: Option<(Vec3, Vec3, Real)> = None;
    for _ in 0..16 {
        let face = b.face(cur)?;
        match b.surface(face.surface)? {
            Surface::Cylinder(c) => {
                let (o, a, r) = (s3(c.origin), d3(c.axis.into_inner()), c.radius);
                if a.cross(dir).len() > 1e-6
                    || removed.iter().any(|&(ro, ra)| coaxial(o, a, ro, ra))
                {
                    return None;
                }
                if let Some((o0, a0, r0)) = cyl {
                    let off = (o - o0) - a0 * (o - o0).dot(a0);
                    if (r - r0).abs() > 1e-6 || off.len() > 1e-6 || a.cross(a0).len() > 1e-6 {
                        return None;
                    }
                } else {
                    cyl = Some((o, a, r));
                }
                pieces.push(cur);
                // On to the straight edge along the axis on the piece's far side.
                let far = face
                    .outer
                    .coedges
                    .iter()
                    .map(|c| c.edge)
                    .filter(|&g| g != via)
                    .find(|&g| {
                        line_ends(b, g).is_some_and(|(p, q)| {
                            (q - p)
                                .normalized()
                                .is_some_and(|d| d.cross(dir).len() < 1e-6)
                                && (p - ea).cross(dir).len() > 1e-6
                        })
                    })?;
                via = far;
                cur = across(b, cur, far)?;
            }
            Surface::Plane(_) if !pieces.is_empty() => {
                let (o, a, r) = cyl?;
                return Some(Crossing {
                    pieces,
                    next: cur,
                    entry: via,
                    radius: r,
                    axis_o: o,
                    axis: a,
                });
            }
            _ => return None,
        }
    }
    None
}

/// The thickness: the nearest planar face facing away from `f` behind it.
fn thickness(b: &BRep, f: FaceId) -> Option<Real> {
    let (n, o) = plane_of(b, f)?;
    b.faces()
        .filter(|(g, _)| *g != f)
        .filter_map(|(g, _)| {
            let (m, q) = plane_of(b, g)?;
            (m.dot(n) < -1.0 + 1e-9).then(|| (o - q).dot(n))
        })
        .filter(|d| *d > 1e-6)
        .min_by(|a, b| a.total_cmp(b))
}

/// Whether two lines (point, unit direction) are one.
fn coaxial(o: Vec3, a: Vec3, o2: Vec3, a2: Vec3) -> bool {
    let off = (o - o2) - a2 * (o - o2).dot(a2);
    a.cross(a2).len() < 1e-6 && off.len() < 1e-6
}

/// A rip of the sheet (spec 5.6 §6.1g): the gap that lets a closed shape
/// unfold. Its face is a face name of the body with a point near it.
#[derive(Clone, Debug, PartialEq)]
pub struct Rip {
    /// The ripped face's name.
    pub face: String,
    /// A point near it (picks the piece when several faces share the name).
    pub hint: Point3,
    /// Where it cuts.
    pub kind: RipKind,
}

/// Where a [`Rip`] cuts. The gap of a line rip runs from `lo` to `hi` along
/// d × n (d the rip's direction, n the face's outward normal): Inventor's
/// default side is `0..gap`, Flip Side `-gap..0`, Both Sides `±gap/2`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RipKind {
    /// Across the face from a point on one of its edges, square to that edge.
    Single {
        /// The point on the edge.
        at: Point3,
        /// Where the gap starts along d × n.
        lo: Real,
        /// Where it ends.
        hi: Real,
    },
    /// Along the line between two points on the face's edges.
    Line {
        /// The start.
        a: Point3,
        /// The end.
        b: Point3,
        /// Where the gap starts along d × n.
        lo: Real,
        /// Where it ends.
        hi: Real,
    },
    /// The whole face removed: a bend's round face.
    Face,
}

/// A rip found on the body: a gap strip (world corners on the picked face's
/// plane, its normal) or a removed bend (its axis); with the material it
/// takes away.
enum RipCut {
    Strip {
        n: Vec3,
        o: Vec3,
        corners: [Vec3; 4],
        volume: Real,
    },
    Bend {
        o: Vec3,
        a: Vec3,
        volume: Real,
    },
}

/// The face named `name` nearest `hint`.
fn face_named(b: &BRep, name: &str, hint: Vec3) -> Option<FaceId> {
    b.faces()
        .filter(|(_, f)| f.name.as_deref() == Some(name))
        .map(|(g, f)| {
            let d = match b.surface(f.surface) {
                Some(Surface::Plane(_)) => plane_of(b, g).map(|(n, o)| (hint - o).dot(n).abs()),
                Some(Surface::Cylinder(c)) => {
                    let (o, a) = (s3(c.origin), d3(c.axis.into_inner()));
                    let w = (hint - o) - a * (hint - o).dot(a);
                    Some((w.len() - c.radius).abs())
                }
                _ => None,
            };
            (g, d.unwrap_or(f64::MAX))
        })
        .min_by(|x, y| x.1.total_cmp(&y.1))
        .map(|(g, _)| g)
}

fn to2(pl: &SPlane, q: Vec3) -> Vec2 {
    let d = q - pl.origin;
    Vec2::new(d.dot(pl.x), d.dot(pl.y))
}

/// The strip (world corners) as a region of `pl`.
fn strip_in(corners: &[Vec3; 4], pl: &SPlane) -> Region2 {
    let pts: Vec<Vec2> = corners.iter().map(|q| to2(pl, *q)).collect();
    Region2 {
        outer: Loop2::polygon(&pts).ccw(),
        holes: Vec::new(),
    }
}

/// `r` minus `strip`.
fn cut_region(r: &Region2, strip: &Region2) -> Result<Vec<Region2>, String> {
    solidcraft_kernel::planar_regions_boolean(
        std::slice::from_ref(r),
        std::slice::from_ref(strip),
        solidcraft_kernel::BoolOp::Cut,
        1e-7,
    )
    .map_err(|e| e.to_string())
}

/// Distance from `p` to the segment a–b.
fn seg_dist(p: Vec2, a: Vec2, b: Vec2) -> Real {
    let ab = b - a;
    let l = ab.dot(ab);
    let t = if l > 0.0 {
        ((p - a).dot(ab) / l).clamp(0.0, 1.0)
    } else {
        0.0
    };
    p.dist(a + ab * t)
}

/// Whether `q` lies on the boundary of `r`.
fn on_boundary(r: &Region2, q: Vec2) -> bool {
    std::iter::once(&r.outer).chain(&r.holes).any(|lp| {
        let poly = lp.polyline(1e-3);
        let n = poly.len();
        (0..n).any(|k| seg_dist(q, poly[k], poly[(k + 1) % n]) < 1e-4)
    })
}

/// The ray parameter where p + t·d meets the segment a–b.
fn ray_hit(p: Vec2, d: Vec2, a: Vec2, b: Vec2) -> Option<Real> {
    let e = b - a;
    let den = d.cross(e);
    if den.abs() < 1e-12 {
        return None;
    }
    let t = (a - p).cross(e) / den;
    let u = (a - p).cross(d) / den;
    (-1e-9..=1.0 + 1e-9).contains(&u).then_some(t)
}

/// What `rip` cuts out of the body `b` of thickness `t`.
fn rip_cut(b: &BRep, rip: &Rip, t: Real) -> Result<RipCut, String> {
    let f = face_named(b, &rip.face, s3(rip.hint))
        .ok_or_else(|| format!("the rip face {} no longer exists", rip.face))?;
    let face = b.face(f).ok_or("face")?;
    let (a2, b2, lo, hi, pl, region) = match rip.kind {
        RipKind::Face => {
            let Some(Surface::Cylinder(c)) = b.surface(face.surface) else {
                return Err(
                    "a Face Extents rip removes a bend: pick one of its round faces".into(),
                );
            };
            let (o, a, r) = (s3(c.origin), d3(c.axis.into_inner()), c.radius);
            // The bend's straight edges along its axis: its angle and length.
            let lines: Vec<(Vec3, Vec3)> = face
                .outer
                .coedges
                .iter()
                .filter_map(|c| line_ends(b, c.edge))
                .filter(|(p, q)| {
                    (*q - *p)
                        .normalized()
                        .is_some_and(|d| d.cross(a).len() < 1e-6)
                })
                .collect();
            let (Some(&(p1, q1)), Some(&(p2, _))) = (lines.first(), lines.get(1)) else {
                return Err("the bend's face has no straight edges along its axis".into());
            };
            let radial = |p: Vec3| ((p - o) - a * (p - o).dot(a)).normalized();
            let theta = radial(p1)
                .zip(radial(p2))
                .map(|(u, v)| u.dot(v).clamp(-1.0, 1.0).acos())
                .ok_or("bend")?;
            // The other skin: a coaxial cylinder one thickness smaller, or larger.
            let smaller = b.faces().any(|(_, g)| {
                matches!(b.surface(g.surface), Some(Surface::Cylinder(c2))
                    if (c2.radius - (r - t)).abs() < 1e-6
                        && coaxial(s3(c2.origin), d3(c2.axis.into_inner()), o, a))
            });
            let mid = if smaller { r - t * 0.5 } else { r + t * 0.5 };
            return Ok(RipCut::Bend {
                o,
                a,
                volume: theta * (q1 - p1).len() * t * mid,
            });
        }
        RipKind::Single { at, lo, hi } => {
            let (n, o) = plane_of(b, f).ok_or("a Single Point rip needs a planar face")?;
            let pl = frame_on(n, o);
            let region = region_of(b, f, &pl)?;
            let at = s3(at);
            if (at - o).dot(n).abs() > 1e-3 {
                return Err("the rip point must lie on the rip face".into());
            }
            let p = to2(&pl, at);
            let poly = region.outer.polyline(1e-3);
            let m = poly.len();
            let (k, dist) = (0..m)
                .map(|k| (k, seg_dist(p, poly[k], poly[(k + 1) % m])))
                .min_by(|x, y| x.1.total_cmp(&y.1))
                .ok_or("the rip face has no outline")?;
            if dist > 1e-3 {
                return Err("the rip point must lie on an edge of the rip face".into());
            }
            // Square to that edge, into the face (the outline runs counter-clockwise).
            let e = (poly[(k + 1) % m] - poly[k]).normalized().ok_or("edge")?;
            let d = Vec2::new(-e.y, e.x);
            let far = (0..m)
                .filter(|&j| j != k)
                .filter_map(|j| ray_hit(p, d, poly[j], poly[(j + 1) % m]))
                .filter(|t| *t > 1e-6)
                .min_by(|x, y| x.total_cmp(y))
                .ok_or("the rip does not cross the face")?;
            (p, p + d * far, lo, hi, pl, region)
        }
        RipKind::Line { a, b: e, lo, hi } => {
            let (n, o) = plane_of(b, f).ok_or("a Point to Point rip needs a planar face")?;
            let pl = frame_on(n, o);
            let region = region_of(b, f, &pl)?;
            let (a, e) = (s3(a), s3(e));
            if (a - o).dot(n).abs() > 1e-3 || (e - o).dot(n).abs() > 1e-3 {
                return Err("the rip points must lie on the rip face".into());
            }
            (to2(&pl, a), to2(&pl, e), lo, hi, pl, region)
        }
    };
    if (hi - lo).is_nan() || hi - lo <= 1e-9 {
        return Err("the rip gap must be positive".into());
    }
    let d = (b2 - a2)
        .normalized()
        .ok_or("the rip's start and end are the same point")?;
    let n = pl.normal();
    let dw = pl.x * d.x + pl.y * d.y;
    let side = dw.cross(n);
    // Past an end on the face's outline, so the gap opens it there; an end
    // inside the face is where the gap stops.
    let ext = 2.0 * t + lo.abs() + hi.abs();
    let past = |q: Vec2| {
        if on_boundary(&region, q) { ext } else { 0.0 }
    };
    let (pa, pb) = (
        pl.to_world(a2) - dw * past(a2),
        pl.to_world(b2) + dw * past(b2),
    );
    let corners = [
        pa + side * lo,
        pb + side * lo,
        pb + side * hi,
        pa + side * hi,
    ];
    let left = cut_region(&region, &strip_in(&corners, &pl))?;
    let area = region.area() - left.iter().map(Region2::area).sum::<Real>();
    if area <= 1e-9 {
        return Err("the rip misses the face's material".into());
    }
    Ok(RipCut::Strip {
        n,
        o: pl.origin,
        corners,
        volume: area * t,
    })
}

/// What [`recognize`] found.
pub enum Recognized {
    /// The sheet: its solid and model.
    Sheet(ExactSolid, SheetModel),
    /// A closed shape: `loops` bends close a loop, so it needs a rip before
    /// it can unfold (Inventor converts it, its flat pattern fails). `model`
    /// is the sheet walked without those bends (no solid of it is made).
    Closed {
        /// The sheet walked without the closing bends.
        model: SheetModel,
        /// How many bends close a loop.
        loops: usize,
    },
}

/// The sheet model of `solid` (an imported sheet metal part) with the planar
/// face containing `face` as its base, cut by `rips`; faces named under `prefix`.
pub fn recognize(
    solid: &ExactSolid,
    prefix: &str,
    rv: &RuleValues,
    face: Point3,
    rips: &[Rip],
) -> Result<Recognized, String> {
    let b = super::super::to_brep(std::slice::from_ref(solid))?;
    let p = s3(face);
    // The base: the planar face whose plane holds the point, nearest inside.
    let base = b
        .faces()
        .filter_map(|(f, _)| {
            let (n, o) = plane_of(&b, f)?;
            let d = (p - o).dot(n).abs();
            (d < 1e-4).then_some(f)
        })
        .find(|&f| {
            plane_of(&b, f).is_some_and(|(n, o)| {
                let pl = frame_on(n, o);
                region_of(&b, f, &pl).is_ok_and(|r| {
                    let poly = r.outer.polyline(1e-3);
                    point_in(&poly, to2(&pl, p))
                })
            })
        })
        .ok_or("pick a planar face of the sheet as its base")?;
    let t = thickness(&b, base)
        .ok_or("no face of the sheet's other side found behind the base face")?;
    let cuts = rips
        .iter()
        .map(|r| rip_cut(&b, r, t))
        .collect::<Result<Vec<_>, _>>()?;
    let removed: Vec<(Vec3, Vec3)> = cuts
        .iter()
        .filter_map(|c| match c {
            RipCut::Bend { o, a, .. } => Some((*o, *a)),
            RipCut::Strip { .. } => None,
        })
        .collect();
    // A face's pieces in `pl` once the rips' strips are cut out of it.
    let pieces_of = |f: FaceId, pl: &SPlane| -> Result<Vec<Region2>, String> {
        let mut reg = vec![region_of(&b, f, pl)?];
        let (nf, of) = plane_of(&b, f).ok_or("face")?;
        for c in &cuts {
            if let RipCut::Strip { n, o, corners, .. } = c
                && n.cross(nf).len() < 1e-6
                && (of - *o).dot(*n).abs() < t + 1e-6
            {
                let strip = strip_in(corners, pl);
                let mut next = Vec::new();
                for r in &reg {
                    next.extend(cut_region(r, &strip)?);
                }
                reg = next;
            }
        }
        Ok(reg)
    };
    let mut rv = *rv;
    rv.thickness = t;
    let (n0, o0) = plane_of(&b, base).ok_or("base face")?;
    // The flat frame: on the base face, z into the material.
    let frame = frame_on(n0 * -1.0, o0);
    let region = {
        let pieces = pieces_of(base, &frame)?;
        let q = to2(&frame, p);
        let k = pieces.iter().position(|r| r.contains(q)).unwrap_or(0);
        pieces
            .into_iter()
            .nth(k)
            .ok_or("the rips leave nothing of the base face")?
    };
    let mut sheet = sd::base_flange(prefix, "rule", &rule_of(&rv), frame, region.clone());
    // Bend faces walked over; per panel face, a point of each piece taken.
    let mut seen: HashSet<FaceId> = HashSet::new();
    let mut taken: std::collections::HashMap<FaceId, Vec<Vec3>> = Default::default();
    taken.insert(base, vec![frame.to_world(region.interior_point())]);
    let mut closing = 0;
    // Panels to walk on from: (face, its panel, its outward normal, the piece, its plane).
    let mut todo: Vec<(FaceId, Option<usize>, Vec3, Region2, SPlane)> =
        vec![(base, None, n0, region, frame)];
    while let Some((f, panel, nf, piece, fpl)) = todo.pop() {
        if sheet.flanges.len() > MAX_PANELS {
            return Err("too many bends for one sheet".into());
        }
        let inv = solidcraft_doc::mat_inverse(&sheet.world(panel)).ok_or("panel frame")?;
        let flat = |q: Vec3| {
            let r = solidcraft_doc::apply_point(&inv, q);
            Vec2::new(r.x, r.y)
        };
        // A point inside the panel, in the flat (to tell its outward side).
        let inside = flat(fpl.to_world(piece.interior_point()));
        for e in straight_edges(&b, f) {
            let Some((ea, eb)) = line_ends(&b, e) else {
                continue;
            };
            // Only the edges of this piece of a ripped face.
            if !on_boundary(&piece, to2(&fpl, (ea + eb) * 0.5)) {
                continue;
            }
            let Some(cr) = crossing(&b, f, e, &removed) else {
                continue;
            };
            if cr.pieces.iter().any(|g| seen.contains(g)) {
                continue;
            }
            let (nn, no) = plane_of(&b, cr.next).ok_or("panel face")?;
            // The next panel's piece: the one the bend meets.
            let pl = frame_on(nn, no);
            let next_piece = {
                let (qa, qb) = line_ends(&b, cr.entry).ok_or("edge")?;
                let m = to2(&pl, (qa + qb) * 0.5);
                pieces_of(cr.next, &pl)?
                    .into_iter()
                    .find(|r| on_boundary(r, m))
            };
            let Some(reg) = next_piece else {
                continue;
            };
            let have = taken
                .get(&cr.next)
                .is_some_and(|ps| ps.iter().any(|q| reg.contains(to2(&pl, *q))));
            if have {
                // A bend back onto a panel already reached: the shape closes
                // (counted once, not again from its other side).
                closing += 1;
                seen.extend(cr.pieces.iter().copied());
                continue;
            }
            // Convex on this skin (the axis inside the material): it turns away from it.
            let mid = (ea + eb) * 0.5;
            let w = (mid - cr.axis_o) - cr.axis * (mid - cr.axis_o).dot(cr.axis);
            let convex = w.dot(nf) > 0.0;
            let radius = if convex { cr.radius - t } else { cr.radius };
            if radius.is_nan() || radius <= -1e-9 {
                return Err(format!(
                    "a bend of radius {:.3} mm is tighter than the sheet ({t:.3} mm)",
                    cr.radius
                ));
            }
            let angle = nf.dot(nn).clamp(-1.0, 1.0).acos();
            if angle.is_nan() || angle <= 1e-6 {
                continue;
            }
            let (mut qa, mut qb) = (flat(ea), flat(eb));
            let mut d = (qb - qa).normalized().ok_or("edge")?;
            if (inside - qa).dot(Vec2::new(d.y, -d.x)) > 0.0 {
                std::mem::swap(&mut qa, &mut qb);
                d = d * -1.0;
            }
            let n = Vec2::new(d.y, -d.x);
            sheet.flanges.push(sd::SheetFlange {
                parent: panel,
                p: snapped(qa),
                d,
                len: qa.dist(qb),
                n,
                angle: if convex { angle } else { -angle },
                radius: radius.max(0.0),
                leg: 0.0,
                ext: (0.0, 0.0),
                slope: (0.0, 0.0),
                tear: (false, false),
                mitre: (Vec::new(), Vec::new()),
                shape: None,
                tag: format!("{prefix}.{}", sheet.flanges.len() + 1),
            });
            let k = sheet.flanges.len() - 1;
            let ba = sheet
                .flanges
                .get(k)
                .map(|fl| sheet.allowance(fl))
                .unwrap_or(0.0);
            // The next panel's piece, through the bend's fold into the flat.
            let mapped = sheet
                .region_to_flat(Some(k), &pl, &reg)
                .map_err(|e| e.to_string())?;
            let pa = snapped(qa);
            let mut shape: Vec<Vec2> = mapped
                .outer
                .polyline(1e-2)
                .into_iter()
                .map(|q| {
                    let off = (q - pa).dot(n) - ba;
                    snapped(if off.abs() < 1e-6 { q - n * off } else { q })
                })
                .collect();
            // No two points closer than the kernel's tolerance (arcs come as fine chords).
            shape.dedup_by(|b, a| a.dist(*b) < 1e-5);
            while shape.len() > 2
                && shape
                    .first()
                    .zip(shape.last())
                    .is_some_and(|(a, b)| a.dist(*b) < 1e-5)
            {
                shape.pop();
            }
            let near = shape
                .iter()
                .map(|q| (*q - pa).dot(n) - ba)
                .fold(f64::MAX, f64::min);
            if near.abs() > 1e-3 {
                return Err(format!(
                    "a bend of radius {radius:.3} mm does not unfold to its panel ({near:.4} mm off): \
                     the sheet is not of constant thickness {t:.3} mm there"
                ));
            }
            let leg = shape
                .iter()
                .map(|q| (*q - pa).dot(n) - ba)
                .fold(0.0, f64::max);
            if let Some(fl) = sheet.flanges.get_mut(k) {
                fl.shape = Some(shape);
                fl.leg = leg;
            }
            for h in &mapped.holes {
                sheet.holes.push(Region2 {
                    outer: h.clone().ccw(),
                    holes: Vec::new(),
                });
            }
            seen.extend(cr.pieces.iter().copied());
            taken
                .entry(cr.next)
                .or_default()
                .push(pl.to_world(reg.interior_point()));
            todo.push((cr.next, Some(k), nn, reg, pl));
        }
    }
    if sheet.flanges.is_empty() {
        return Err("no round bends found next to the base face".into());
    }
    // Edge names as a Convert's: the base outline `E<k>`.
    let pts = sheet.base.outer.ccw().polyline(1e-3);
    let n = pts.len();
    sheet.base_curves = (0..n)
        .filter_map(|k| {
            let (a, b) = (*pts.get(k)?, *pts.get((k + 1) % n)?);
            (a.dist(b) > 1e-9).then(|| (format!("E{}", k + 1), a, b))
        })
        .collect();
    if closing > 0 {
        return Ok(Recognized::Closed {
            model: SheetModel(Arc::new(sheet)),
            loops: closing,
        });
    }
    let mut s = named_solid(&sheet, prefix, false)?;
    // The rips' material is not in the sheet.
    let taken_away: Real = cuts
        .iter()
        .map(|c| match c {
            RipCut::Strip { volume, .. } | RipCut::Bend { volume, .. } => *volume,
        })
        .sum();
    let (want, got) = (
        solid.volume_at(1e-3).map(|v| v - taken_away),
        s.volume_at(1e-3),
    );
    if let (Some(w), Some(g)) = (want, got)
        && (w - g).abs() > w.abs() * 2e-3
    {
        return Err(format!(
            "the sheet found ({g:.1} mm³) is not the body ({w:.1} mm³): not a sheet of constant \
             thickness {t:.3} mm, or features the conversion does not know yet (cut-outs across \
             bends, formed features, bends that are not round)"
        ));
    }
    s.name = solid.name.clone();
    Ok(Recognized::Sheet(s, SheetModel(Arc::new(sheet))))
}

fn point_in(poly: &[Vec2], p: Vec2) -> bool {
    let mut inside = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + n - 1) % n]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
    }
    inside
}

/// A Single Point / Point to Point rip of a sheet that unfolds already (not
/// closed): its gap cut through the face as a sheet metal Cut does, kept in
/// the flat pattern.
pub fn rip_open(
    m: &SheetModel,
    prefix: &str,
    solid: &ExactSolid,
    rip: &Rip,
) -> Result<(ExactSolid, SheetModel), String> {
    let b = super::super::to_brep(std::slice::from_ref(solid))?;
    let RipCut::Strip { n, o, corners, .. } = rip_cut(&b, rip, m.0.t)? else {
        return Err("not yet: a Face Extents rip of a sheet that is not closed".into());
    };
    let pl = frame_on(n, o);
    let strip = strip_in(&corners, &pl);
    let mut next = (*m.0).clone();
    super::guarded_sheet("Rip", || {
        next.cut_through(&pl, std::slice::from_ref(&strip), sd::CutMode::Face)
            .map(|_| ())
            .map_err(|e| e.to_string())
    })?;
    let s = named_solid(&next, prefix, false)?;
    Ok((s, SheetModel(Arc::new(next))))
}
