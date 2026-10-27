//! **Project Cut Edges** and **Project Flat Pattern** (Sketch → Create →
//! Project Geometry ▾, `docs/spec/1.8-project-geometry.md`): geometry the
//! model has but no edge to click, as fixed sketch geometry. Inventor's cut
//! edges are a snapshot, not associative: they stay where they are when the
//! model changes (project them again to update); so are our flat patterns.
//!
//! The section's pieces come from each face's display triangles (which faces
//! the plane crosses, where each crossing starts and ends), their geometry
//! from the face's exact surface: a plane gives a line, a cylinder square to
//! the sketch or a sphere a circle / arc, anything else an interpolation
//! spline through [`super::link::CURVE_POINTS`] points of the crossing. The
//! joints are then made exact (line × line, line × circle, circle × circle).
//! Splitting the solid with the kernel and converting the part took 2.5 s
//! for a plate with 36 holes; this takes milliseconds. The flat
//! pattern is the sheet's unfolded outline and cut-outs
//! ([`ksm::flat_profile`]) laid in its base panel's plane, as Unfold leaves
//! it, with the bend centre lines as construction lines.

use std::collections::HashMap;

use crate::kernel::exact::sheet_metal as ksm;
use crate::kernel::{BRep, Plane, Point3, Real, Surface, TessellationOptions, Tolerance};
use crate::sketch::{EntityId, Point2, PointId, Sketch, Style};

use super::{PartDocument, PartError};

/// Points closer than this (mm, in the sketch) are one point.
const JOIN: Real = 1e-6;

/// A piece of model geometry to draw into a sketch (model coordinates).
#[derive(Clone, Debug)]
enum Piece {
    Line(Point3, Point3),
    /// An arc from `start` to `end` through `mid`.
    Arc {
        center: Point3,
        start: Point3,
        mid: Point3,
        end: Point3,
    },
    Circle {
        center: Point3,
        radius: Real,
    },
    /// Any other curve (closed when its ends meet).
    Curve(Vec<Point3>),
}

impl PartDocument {
    /// **Project Cut Edges** into sketch `index`: the section of every solid
    /// with the sketch plane as fixed lines, arcs, circles and splines
    /// (loops share their points). Returns the new curves; nothing changes
    /// when the plane cuts no solid.
    pub fn project_cut_edges(&mut self, index: usize) -> Result<Vec<EntityId>, PartError> {
        let fail = |m: String| PartError::Feature(format!("Project Cut Edges: {m}"));
        let ps = self.sketches.get(index).ok_or(PartError::NoSketch(index))?;
        let plane = ps.sketch.plane.clone();
        let pieces = section_pieces(&self.body, &plane);
        if pieces.is_empty() {
            return Err(fail("the sketch plane does not cut the part".into()));
        }
        self.add_pieces(index, &pieces, &[]).map_err(fail)
    }

    /// **Project Flat Pattern** of sheet `body` into sketch `index`: its
    /// outline and cut-outs (lines, arcs, circles; other curves as splines)
    /// and its bend centre lines (construction), laid in the base panel's
    /// plane on the top or bottom face, whichever the sketch is on (the
    /// nearer one otherwise). The sketch must be parallel to the base
    /// panel. Returns the new curves.
    pub fn project_flat_pattern(
        &mut self,
        index: usize,
        body: &str,
    ) -> Result<Vec<EntityId>, PartError> {
        let fail = |m: String| PartError::Feature(format!("Project Flat Pattern: {m}"));
        let plane = self
            .sketches
            .get(index)
            .ok_or(PartError::NoSketch(index))?
            .sketch
            .plane
            .clone();
        let st = self
            .sheet_models
            .get(body)
            .ok_or_else(|| fail(format!("{body} is not a sheet metal body")))?;
        let t = ksm::thickness(&st.model);
        // The flat's frame: origin, x and y as the sketch would see them.
        let at = |p: [Real; 2], z: Real| ksm::flat_point(&st.model, p, z);
        let (o, ex, ey) = (
            at([0.0, 0.0], 0.0),
            at([1.0, 0.0], 0.0),
            at([0.0, 1.0], 0.0),
        );
        let n = (ex - o).cross(&(ey - o));
        let n = n
            .try_normalize(1e-12)
            .ok_or_else(|| fail("the flat pattern has no plane".into()))?;
        if n.cross(&plane.normal).norm() > 1e-6 {
            return Err(fail(
                "the sketch is not parallel to the sheet's base panel (sketch on its top or bottom face)"
                    .into(),
            ));
        }
        // The face of the sheet nearer the sketch plane.
        let dist = |z: Real| (at([0.0, 0.0], z) - plane.origin).dot(&plane.normal).abs();
        let z = if dist(t) < dist(0.0) { t } else { 0.0 };
        let loops = ksm::flat_profile(&st.model).map_err(fail)?;
        let p = |q: [Real; 2]| at(q, z);
        let mut pieces = Vec::new();
        for seg in loops.iter().flatten() {
            pieces.push(match seg {
                ksm::FlatSeg::Line(a, b) => Piece::Line(p(*a), p(*b)),
                ksm::FlatSeg::Arc {
                    center,
                    radius,
                    start,
                    sweep,
                } => {
                    let on =
                        |a: Real| p([center[0] + radius * a.cos(), center[1] + radius * a.sin()]);
                    Piece::Arc {
                        center: p(*center),
                        start: on(*start),
                        mid: on(start + sweep / 2.0),
                        end: on(start + sweep),
                    }
                }
                ksm::FlatSeg::Circle { center, radius } => Piece::Circle {
                    center: p(*center),
                    radius: *radius,
                },
                ksm::FlatSeg::Curve(pts) => Piece::Curve(pts.iter().map(|q| p(*q)).collect()),
            });
        }
        let bends: Vec<Piece> = ksm::bends(&st.model)
            .into_iter()
            .map(|b| Piece::Line(p(b.start), p(b.end)))
            .collect();
        self.add_pieces(index, &pieces, &bends).map_err(fail)
    }

    /// Draws `pieces` (and `construction` pieces) into sketch `index` as
    /// fixed geometry; on failure the sketch is left as it was.
    fn add_pieces(
        &mut self,
        index: usize,
        pieces: &[Piece],
        construction: &[Piece],
    ) -> Result<Vec<EntityId>, String> {
        let ps = self.sketches.get_mut(index).ok_or("no such sketch")?;
        let before = ps.sketch.clone();
        let mut pen = Pen { points: Vec::new() };
        let mut out = Vec::new();
        let r = (|| {
            for (list, cons) in [(pieces, false), (construction, true)] {
                for piece in list {
                    if let Some(c) = pen.draw(&mut ps.sketch, piece)? {
                        if cons {
                            ps.sketch
                                .set_style(c, Style::Construction)
                                .map_err(|e| e.to_string())?;
                        }
                        out.push(c);
                    }
                }
            }
            Ok::<(), String>(())
        })();
        if let Err(e) = r {
            ps.sketch = before;
            return Err(e);
        }
        Ok(out)
    }
}

/// Draws pieces, sharing the points where their ends meet.
struct Pen {
    points: Vec<(Point2, PointId)>,
}

impl Pen {
    fn point(&mut self, sk: &mut Sketch, p: Point2) -> PointId {
        if let Some(&(_, id)) = self.points.iter().find(|(q, _)| (q - p).norm() <= JOIN) {
            return id;
        }
        let id = sk.add_fixed_point(p);
        self.points.push((p, id));
        id
    }

    /// The curve of `piece` (`None`: it has no length in the sketch).
    fn draw(&mut self, sk: &mut Sketch, piece: &Piece) -> Result<Option<EntityId>, String> {
        let plane: Plane = sk.plane.clone();
        let pr = |p: &Point3| plane.project(p);
        let err = |e: crate::sketch::SketchError| e.to_string();
        Ok(Some(match piece {
            Piece::Line(a, b) => {
                let (a, b) = (pr(a), pr(b));
                if (a - b).norm() <= JOIN {
                    return Ok(None);
                }
                let (pa, pb) = (self.point(sk, a), self.point(sk, b));
                sk.add_line_between(pa, pb).map_err(err)?
            }
            Piece::Circle { center, radius } => {
                let pc = sk.add_fixed_point(pr(center));
                let id = sk.add_circle_at(pc, *radius).map_err(err)?;
                let _ = sk.set_radius_fixed(id, true);
                id
            }
            Piece::Arc {
                center,
                start,
                mid,
                end,
            } => {
                let (c, a, m, b) = (pr(center), pr(start), pr(mid), pr(end));
                // Counter-clockwise in the sketch from one end to the other
                // through the middle.
                let ang = |p: Point2| (p.y - c.y).atan2(p.x - c.x);
                let ccw =
                    |s: Point2, t: Point2| (ang(t) - ang(s)).rem_euclid(std::f64::consts::TAU);
                let (s, t) = if ccw(a, m) > ccw(a, b) {
                    (b, a)
                } else {
                    (a, b)
                };
                let pc = sk.add_fixed_point(c);
                let (ps, pt) = (self.point(sk, s), self.point(sk, t));
                sk.add_arc(pc, ps, pt).map_err(err)?
            }
            Piece::Curve(pts) => {
                if pts.len() < 2 {
                    return Ok(None);
                }
                let closed = (pts[0] - pts[pts.len() - 1]).norm() <= JOIN;
                let k = super::link::CURVE_POINTS;
                let n = pts.len() - 1;
                let mut ids = Vec::new();
                for i in 0..k {
                    let p = pr(&pts[((i * n) / (k - 1)).min(n)]);
                    let id = if i == 0 || i == k - 1 {
                        self.point(sk, p)
                    } else {
                        sk.add_fixed_point(p)
                    };
                    if ids.last() != Some(&id) {
                        ids.push(id);
                    }
                }
                if closed && ids.first() != ids.last() {
                    ids.push(ids[0]);
                }
                if ids.len() < 2 {
                    return Ok(None);
                }
                sk.add_interp_spline(ids).map_err(err)?
            }
        }))
    }
}

/// What a face's crossing with the plane is, exactly (sketch coordinates).
#[derive(Clone, Copy, Debug)]
enum Kind {
    Line,
    Circle { c: Point2, r: Real },
    Curve,
}

/// One crossing of a face with the plane: its kind and its points from the
/// display triangles (sketch coordinates), ends first and last.
struct Chain {
    kind: Kind,
    pts: Vec<Point2>,
    closed: bool,
}

/// The section of `body`'s faces by `plane` as pieces (model coordinates).
fn section_pieces(body: &BRep, plane: &Plane) -> Vec<Piece> {
    let tol = Tolerance::default();
    let opts = TessellationOptions::default();
    let n = plane.normal.into_inner();
    let mut chains: Vec<Chain> = Vec::new();
    for (fid, face) in body.faces() {
        let mesh = match &face.mesh {
            Some(m) => (**m).clone(),
            None => match body.face_mesh(fid, &opts) {
                Ok(m) => m,
                Err(_) => continue,
            },
        };
        let d: Vec<Real> = mesh
            .positions
            .iter()
            .map(|p| (p - plane.origin).dot(&n))
            .collect();
        // A face in the plane itself has no crossing (its edges are the
        // section's, from its neighbours).
        if d.iter().all(|x| x.abs() <= tol.linear) {
            continue;
        }
        let side = |k: u32| d[k as usize] > 0.0;
        let cross = |a: u32, b: u32| {
            let (a, b) = (a.min(b), a.max(b));
            let (pa, pb) = (mesh.positions[a as usize], mesh.positions[b as usize]);
            let (da, db) = (d[a as usize], d[b as usize]);
            plane.project(&(pa + (pb - pa) * (da / (da - db))))
        };
        let mut segs: Vec<(Point2, Point2)> = Vec::new();
        for t in &mesh.triangles {
            let mut hit = Vec::with_capacity(2);
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                if side(a) != side(b) {
                    hit.push(cross(a, b));
                }
            }
            if let [p, q] = hit[..]
                && (p - q).norm() > 1e-12
            {
                segs.push((p, q));
            }
        }
        if segs.is_empty() {
            continue;
        }
        let kind = surface_kind(body, face, plane);
        for (pts, closed) in chain_segments(&segs) {
            chains.push(Chain { kind, pts, closed });
        }
    }
    // The joints: chain ends that meet (the same triangle edge of two
    // faces' meshes), made exact from the two curves.
    let mut ends: Vec<(usize, bool, Point2)> = Vec::new();
    for (k, c) in chains.iter().enumerate() {
        if !c.closed && c.pts.len() >= 2 {
            ends.push((k, false, c.pts[0]));
            ends.push((k, true, c.pts[c.pts.len() - 1]));
        }
    }
    let near = 1e-5;
    let mut fixed: HashMap<(usize, bool), Point2> = HashMap::new();
    for i in 0..ends.len() {
        let (ka, ea, pa) = ends[i];
        let Some(&(kb, eb, _)) = ends
            .iter()
            .find(|&&(kb, eb, pb)| (kb, eb) != (ka, ea) && (pb - pa).norm() <= near)
        else {
            continue;
        };
        if fixed.contains_key(&(ka, ea)) {
            continue;
        }
        let p = joint(&chains[ka], &chains[kb], pa);
        fixed.insert((ka, ea), p);
        fixed.insert((kb, eb), p);
    }
    let w = |p: Point2| plane.point_at(p.x, p.y);
    let mut out = Vec::new();
    for (k, c) in chains.iter().enumerate() {
        let a = fixed.get(&(k, false)).copied().unwrap_or(c.pts[0]);
        let b = fixed
            .get(&(k, true))
            .copied()
            .unwrap_or(c.pts[c.pts.len() - 1]);
        match c.kind {
            Kind::Circle { c: cc, r } if c.closed => out.push(Piece::Circle {
                center: w(cc),
                radius: r,
            }),
            Kind::Circle { c: cc, r } => {
                let m = c.pts[c.pts.len() / 2];
                let m = cc + (m - cc).normalize() * r;
                out.push(Piece::Arc {
                    center: w(cc),
                    start: w(a),
                    mid: w(m),
                    end: w(b),
                });
            }
            Kind::Line if !c.closed => out.push(Piece::Line(w(a), w(b))),
            _ => {
                let mut pts: Vec<Point3> = c.pts.iter().map(|&p| w(p)).collect();
                if !c.closed {
                    let last = pts.len() - 1;
                    pts[0] = w(a);
                    pts[last] = w(b);
                }
                out.push(Piece::Curve(pts));
            }
        }
    }
    out
}

/// The exact curve the plane cuts out of `face`.
fn surface_kind(body: &BRep, face: &crate::kernel::Face, plane: &Plane) -> Kind {
    let tol = Tolerance::default();
    let id = face.facet_of.unwrap_or(face.surface);
    match body.surface(id) {
        Some(Surface::Plane(_)) => Kind::Line,
        Some(Surface::Cylinder(c)) if tol.are_parallel(&c.axis, &plane.normal) => Kind::Circle {
            c: plane.project(&c.origin),
            r: c.radius,
        },
        Some(Surface::Sphere(s)) => {
            let d = (s.center - plane.origin).dot(&plane.normal);
            let r2 = s.radius * s.radius - d * d;
            if r2 > 0.0 {
                Kind::Circle {
                    c: plane.project(&s.center),
                    r: r2.sqrt(),
                }
            } else {
                Kind::Curve
            }
        }
        _ => Kind::Curve,
    }
}

/// Joins segments that share ends into polylines (closed ones repeat their
/// first point at the end).
fn chain_segments(segs: &[(Point2, Point2)]) -> Vec<(Vec<Point2>, bool)> {
    let key = |p: Point2| ((p.x * 1e7).round() as i64, (p.y * 1e7).round() as i64);
    let mut at: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, (a, b)) in segs.iter().enumerate() {
        at.entry(key(*a)).or_default().push(i);
        at.entry(key(*b)).or_default().push(i);
    }
    let mut used = vec![false; segs.len()];
    let mut out = Vec::new();
    // Start at an end of an open chain first (a point with one segment).
    let mut order: Vec<usize> = (0..segs.len()).collect();
    order.sort_by_key(|&i| {
        let (a, b) = segs[i];
        let lone = |p| at.get(&key(p)).is_some_and(|v| v.len() == 1);
        !(lone(a) || lone(b))
    });
    for s in order {
        if used[s] {
            continue;
        }
        used[s] = true;
        let (a, b) = segs[s];
        let lone = |p| at.get(&key(p)).is_some_and(|v| v.len() == 1);
        let mut pts = if lone(b) && !lone(a) {
            vec![b, a]
        } else {
            vec![a, b]
        };
        // Walk on from the last point.
        while let Some(&last) = pts.last() {
            let Some(&next) = at
                .get(&key(last))
                .and_then(|v| v.iter().find(|&&i| !used[i]))
            else {
                break;
            };
            used[next] = true;
            let (p, q) = segs[next];
            pts.push(if key(p) == key(last) { q } else { p });
        }
        let closed = pts.len() > 2 && key(pts[0]) == key(pts[pts.len() - 1]);
        out.push((pts, closed));
    }
    out
}

/// The exact meeting point of two crossings near `p`.
fn joint(a: &Chain, b: &Chain, p: Point2) -> Point2 {
    let line = |c: &Chain| {
        let (s, e) = (c.pts[0], c.pts[c.pts.len() - 1]);
        let d = e - s;
        (d.norm() > 1e-12).then(|| (s, d.normalize()))
    };
    let on_line_circle = |(s, u): (Point2, crate::sketch::Vector2), c: Point2, r: Real| {
        // |s + t u - c| = r, the root nearest p; a tangent line (no root or
        // a double one) touches at the foot of the centre.
        let f = s - c;
        let bq = f.dot(&u);
        let disc = bq * bq - (f.norm_squared() - r * r);
        let t0 = (p - s).dot(&u);
        let t = if disc <= 0.0 {
            -bq
        } else {
            let (t1, t2) = (-bq - disc.sqrt(), -bq + disc.sqrt());
            if (t1 - t0).abs() < (t2 - t0).abs() {
                t1
            } else {
                t2
            }
        };
        s + u * t
    };
    let q = match (a.kind, b.kind) {
        (Kind::Line, Kind::Line) => match (line(a), line(b)) {
            (Some((s1, u1)), Some((s2, u2))) => {
                let den = u1.perp(&u2);
                if den.abs() < 1e-12 {
                    p
                } else {
                    s1 + u1 * ((s2 - s1).perp(&u2) / den)
                }
            }
            _ => p,
        },
        (Kind::Line, Kind::Circle { c, r }) => line(a).map_or(p, |l| on_line_circle(l, c, r)),
        (Kind::Circle { c, r }, Kind::Line) => line(b).map_or(p, |l| on_line_circle(l, c, r)),
        (Kind::Circle { c: c1, r: r1 }, Kind::Circle { c: c2, r: r2 }) => {
            let d = c2 - c1;
            let l = d.norm();
            if l < 1e-12 {
                p
            } else {
                let x = (l * l + r1 * r1 - r2 * r2) / (2.0 * l);
                let h2 = r1 * r1 - x * x;
                let u = d / l;
                let m = c1 + u * x;
                if h2 <= 0.0 {
                    m
                } else {
                    let v = crate::sketch::Vector2::new(-u.y, u.x) * h2.sqrt();
                    let (q1, q2) = (m + v, m - v);
                    if (q1 - p).norm() < (q2 - p).norm() {
                        q1
                    } else {
                        q2
                    }
                }
            }
        }
        _ => p,
    };
    // Never move a joint far (a wrong pairing would distort the loop).
    if (q - p).norm() < 0.1 { q } else { p }
}
