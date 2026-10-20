//! Sweeps and lofts through the exact kernel (`docs/spec/5.1-sweep.md`,
//! `docs/spec/5.2-loft.md`): SolidCraft's `sweep`, `sweep_path`, `loft` and
//! `loft_to_point`, plus a loft through placed copies of one profile
//! ([`loft_frames`]) for the options the kernel has no sweep for (taper,
//! twist, a guide rail, Parallel orientation).
//!
//! Face names come from the kernel's history: `<prefix>:Side:<segment>`,
//! `<prefix>:Start`, `<prefix>:End`; a loft's later sections name their sides
//! `<prefix>:Side:s<k>.<segment>`.

use super::{
    ExactProfile, ExactSolid, FaceOrigin, Plane, Point3, Real, Vector3, d3, guarded, names_from,
    plane_of, sg, sk, v3, w3,
};

/// One piece of a sweep path, in model coordinates.
#[derive(Clone, Debug, PartialEq)]
pub enum PathPiece {
    /// Straight from `a` to `b`.
    Line {
        /// Start.
        a: Point3,
        /// End.
        b: Point3,
    },
    /// From `a` about the axis through `center` (right hand) by `angle`.
    Arc {
        /// Start.
        a: Point3,
        /// Centre.
        center: Point3,
        /// Unit axis.
        axis: Vector3,
        /// Signed angle (radians).
        angle: Real,
    },
    /// A polyline (a spline or another curve followed by its points).
    Points(Vec<Point3>),
}

impl PathPiece {
    /// Its start point.
    pub fn start(&self) -> Option<Point3> {
        match self {
            PathPiece::Line { a, .. } | PathPiece::Arc { a, .. } => Some(*a),
            PathPiece::Points(p) => p.first().copied(),
        }
    }
}

/// A placed copy of a profile: sketch coordinates `(u, v)` go to
/// `origin + u·x + v·y` (`x`, `y` need not be unit or perpendicular: a
/// scaled or stretched section).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    /// Image of the sketch origin.
    pub origin: Point3,
    /// Image of the sketch x axis.
    pub x: Vector3,
    /// Image of the sketch y axis.
    pub y: Vector3,
}

fn side_names(names: &[String], prefix: &str) -> impl Fn(&FaceOrigin) -> Option<String> {
    let names = names.to_vec();
    let prefix = prefix.to_owned();
    move |o| match o {
        FaceOrigin::Profile { segment } => {
            names.get(*segment).map(|n| format!("{prefix}:Side:{n}"))
        }
        FaceOrigin::Cap { end } => Some(format!("{prefix}:{}", if *end { "End" } else { "Start" })),
        _ => None,
    }
}

/// Names for a body without a history: faces in kernel order, the planar
/// ones on the start plane `Start`, the rest sides by number.
fn fallback_names(body: &sk::Body, start: &Plane, prefix: &str) -> Vec<Option<String>> {
    let faces = guarded("faces", || body.faces(1e-3)).unwrap_or_default();
    let n = start.normal.into_inner();
    let mut side = 0;
    faces
        .iter()
        .map(|f| {
            let c = w3(f.centroid);
            let on = ((c - start.origin.coords).dot(&n)).abs() < 1e-6
                && f.plane_normal
                    .is_some_and(|pn| w3(pn).cross(&n).norm() < 1e-6);
            if on {
                Some(format!("{prefix}:Start"))
            } else {
                side += 1;
                Some(format!("{prefix}:Side:{}", side - 1))
            }
        })
        .collect()
}

/// Sweeps `prof` (on `plane`) along `path`, keeping it at the same angle to
/// the path (Inventor's Orientation Path): lines translate it, arcs turn it
/// (exact walls), a path with corners or polylines is followed point by
/// point (mitred corners, smooth B-spline walls where it is smooth).
pub fn sweep(
    plane: &Plane,
    prof: &ExactProfile,
    path: &[PathPiece],
    prefix: &str,
) -> Result<ExactSolid, String> {
    let pl = plane_of(plane)?;
    if path.is_empty() {
        return Err("the path is empty".into());
    }
    let exact = path.iter().all(|p| !matches!(p, PathPiece::Points(_)));
    let body = if exact {
        let segs: Vec<sk::PathSeg> = path
            .iter()
            .filter_map(|p| match p {
                PathPiece::Line { a, b } => Some(sk::PathSeg::Line { a: v3(a), b: v3(b) }),
                PathPiece::Arc {
                    a,
                    center,
                    axis,
                    angle,
                } => Some(sk::PathSeg::Arc {
                    a: v3(a),
                    center: v3(center),
                    axis: d3(axis),
                    angle: *angle,
                }),
                PathPiece::Points(_) => None,
            })
            .collect();
        guarded("sweep", || sk::sweep(&pl, &prof.region, &segs))?
    } else {
        let pts = path_points(path);
        // A whole circle is one edge with a single vertex: the kernel's
        // point-path sweep builds walls between the segments' start vertices
        // and panicked on it ("This wire is not simple"). Quarter arcs work.
        let prof = split_closed_arcs(prof);
        let body = guarded("sweep", || sk::sweep_path(&pl, &prof.region, &pts))?;
        return named(body, plane, &prof.names, prefix);
    };
    named(body, plane, &prof.names, prefix)
}

/// The profile with every arc cut into pieces of at most a quarter turn
/// (names repeated per piece, as [`super::profile`] does for B-splines).
fn split_closed_arcs(prof: &ExactProfile) -> ExactProfile {
    let id = [[1.0, 0.0], [0.0, 1.0]];
    let zero = sg::Vec2::new(0.0, 0.0);
    let (on, hn) = split_names(prof);
    let (outer, counts) = map_loop(&prof.region.outer, id, zero, true);
    let mut names = expand(&on, &counts);
    let mut holes = Vec::with_capacity(prof.region.holes.len());
    for (h, n) in prof.region.holes.iter().zip(&hn) {
        let (segs, c) = map_loop(h, id, zero, true);
        names.extend(expand(n, &c));
        holes.push(sg::Loop2 { segs });
    }
    ExactProfile {
        region: sg::Region2 {
            outer: sg::Loop2 { segs: outer },
            holes,
        },
        names,
        starts: prof.starts.clone(),
    }
}

/// Where a profile swept along `path` (kept at its angle to the path, as
/// the kernel does: rotation-minimising frames) would fold over itself: the
/// first path station whose bend radius is smaller than the profile's reach
/// towards the bend's centre. `Err` names the place in user terms (Inventor
/// refuses self-intersecting sweeps too).
pub fn check_bend(plane: &Plane, prof: &ExactProfile, path: &[PathPiece]) -> Result<(), String> {
    let pts: Vec<Point3> = path_points(path)
        .into_iter()
        .map(|p| Point3::new(p.x, p.y, p.z))
        .collect();
    let n = pts.len();
    if n < 3 {
        return Ok(());
    }
    let start = pts[0];
    // Points around the profile's boundary, as vectors from the path start.
    let mut q: Vec<Vector3> = Vec::new();
    for lp in std::iter::once(&prof.region.outer).chain(&prof.region.holes) {
        for s in &lp.segs {
            for j in 0..16 {
                let p = s.point_at(j as Real / 16.0);
                q.push(plane.point_at(p.x, p.y) - start);
            }
        }
    }
    let size = q.iter().map(|v| v.norm()).fold(0.0, Real::max).max(1e-9);
    let tangent = |i: usize| -> Option<Vector3> {
        let a = pts[i.saturating_sub(1)];
        let b = pts[(i + 1).min(n - 1)];
        (b - a).try_normalize(1e-12)
    };
    let reflect = |v: Vector3, w: Vector3| -> Vector3 {
        let c = w.dot(&w);
        if c > 1e-24 {
            v - w * (2.0 / c * w.dot(&v))
        } else {
            v
        }
    };
    let Some(mut t) = tangent(0) else {
        return Ok(());
    };
    let mut along = 0.0;
    for i in 1..n - 1 {
        // Carry the profile from station i - 1 to i (double reflection).
        let v1 = pts[i] - pts[i - 1];
        along += v1.norm();
        let Some(tn) = tangent(i) else { continue };
        let tl = reflect(t, v1);
        let v2 = tn - tl;
        for v in q.iter_mut() {
            *v = reflect(reflect(*v, v1), v2);
        }
        t = tn;
        // The bend at station i: the circle through it and its neighbours
        // (a corner is mitred by the kernel, not bent).
        let (a, b) = (pts[i - 1] - pts[i], pts[i + 1] - pts[i]);
        let (la, lb) = (a.norm(), b.norm());
        if la < 1e-6 || lb < 1e-6 {
            continue;
        }
        let turn = (-a.dot(&b) / (la * lb)).clamp(-1.0, 1.0).acos();
        if turn > 0.35 || turn < 1e-9 {
            continue;
        }
        let axb = a.cross(&b);
        let d = 2.0 * axb.norm_squared();
        if d < 1e-30 {
            continue;
        }
        let c = (b * a.norm_squared() - a * b.norm_squared()).cross(&axb) / d;
        let r = c.norm();
        let Some(towards) = c.try_normalize(1e-15) else {
            continue;
        };
        let reach = q.iter().map(|v| v.dot(&towards)).fold(Real::MIN, Real::max);
        if reach >= r - 1e-6 * size {
            return Err(format!(
                "the profile is larger than the path's bend: {along:.2} mm along the path it \
                 bends with radius {r:.2} mm, but the profile reaches {reach:.2} mm towards the \
                 bend, so the sweep would cross itself (use a smaller profile or a gentler path)"
            ));
        }
    }
    Ok(())
}

/// Every point of a path, in order (arcs within 0.01 mm of their chords).
pub fn path_points(path: &[PathPiece]) -> Vec<sg::Vec3> {
    let mut pts: Vec<sg::Vec3> = Vec::new();
    let mut push = |p: sg::Vec3| {
        if pts.last().is_none_or(|l: &sg::Vec3| l.dist(p) > 1e-9) {
            pts.push(p);
        }
    };
    for piece in path {
        match piece {
            PathPiece::Line { a, b } => {
                push(v3(a));
                push(v3(b));
            }
            PathPiece::Arc {
                a,
                center,
                axis,
                angle,
            } => {
                let r = (a - center).norm();
                let step = if r > 1e-2 {
                    2.0 * (1.0 - 1e-2 / r).acos()
                } else {
                    angle.abs()
                };
                let n = ((angle.abs() / step.max(1e-6)).ceil() as usize).clamp(1, 512);
                let k = nalgebra::Unit::new_normalize(*axis);
                for i in 0..=n {
                    let rot =
                        nalgebra::Rotation3::from_axis_angle(&k, angle * i as Real / n as Real);
                    push(v3(&(center + rot * (a - center))));
                }
            }
            PathPiece::Points(p) => p.iter().for_each(|q| push(v3(q))),
        }
    }
    pts
}

fn named(
    body: sk::Body,
    plane: &Plane,
    names: &[String],
    prefix: &str,
) -> Result<ExactSolid, String> {
    let names_of = side_names(names, prefix);
    let mut face_names = names_from(&body, &[], &names_of);
    if face_names.iter().all(Option::is_none) {
        face_names = fallback_names(&body, plane, prefix);
    }
    Ok(ExactSolid {
        sheet: false,
        name: Some(prefix.to_owned()),
        body,
        face_names,
        profiles: Vec::new(),
    })
}

// ---------------------------------------------------------------- lofts ---

/// A section of a loft in the kernel's form: its plane, region and the
/// names of its segments (outer loop, then each hole).
struct Section {
    plane: sg::Plane,
    outer: sg::Loop2,
    outer_names: Vec<String>,
    holes: Vec<(sg::Loop2, Vec<String>)>,
}

/// The outer loop's and the holes' segment names of a profile (the profile
/// numbers them outer loop first).
fn split_names(prof: &ExactProfile) -> (Vec<String>, Vec<Vec<String>>) {
    let mut at = 0;
    let mut take = |n: usize| {
        let v: Vec<String> = prof.names.iter().skip(at).take(n).cloned().collect();
        at += n;
        v
    };
    let outer = take(prof.region.outer.segs.len());
    let holes = prof
        .region
        .holes
        .iter()
        .map(|h| take(h.segs.len()))
        .collect();
    (outer, holes)
}

/// An arc as rational quadratic pieces of at most 90° (exact, and unchanged
/// in form by any linear map).
fn arc_conics(center: sg::Vec2, radius: Real, start: Real, sweep: Real) -> Vec<sg::Seg2> {
    let n = ((sweep.abs() / std::f64::consts::FRAC_PI_2).ceil() as usize).max(1);
    let d = sweep / n as Real;
    let at = |t: Real| sg::Vec2::new(center.x + radius * t.cos(), center.y + radius * t.sin());
    (0..n)
        .map(|i| {
            let t0 = start + d * i as Real;
            let t1 = t0 + d;
            let w = (d / 2.0).cos();
            let tm = (t0 + t1) / 2.0;
            let apex = sg::Vec2::new(
                center.x + radius / w * tm.cos(),
                center.y + radius / w * tm.sin(),
            );
            sg::Seg2::Conic {
                a: at(t0),
                apex,
                b: at(t1),
                w,
            }
        })
        .collect()
}

/// A loop mapped by the linear map `m` (2×2, row-major) plus `t`; arcs stay
/// arcs when `similar` (a rotation and uniform scale), else become conics.
/// Returns the segments and how many each original segment became.
fn map_loop(
    lp: &sg::Loop2,
    m: [[Real; 2]; 2],
    t: sg::Vec2,
    similar: bool,
) -> (Vec<sg::Seg2>, Vec<usize>) {
    let f = |p: sg::Vec2| {
        sg::Vec2::new(
            m[0][0] * p.x + m[0][1] * p.y + t.x,
            m[1][0] * p.x + m[1][1] * p.y + t.y,
        )
    };
    let det = m[0][0] * m[1][1] - m[0][1] * m[1][0];
    let mut out = Vec::new();
    let mut counts = Vec::new();
    for s in &lp.segs {
        let before = out.len();
        match *s {
            sg::Seg2::Line { a, b } => out.push(sg::Seg2::Line { a: f(a), b: f(b) }),
            sg::Seg2::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                if similar {
                    let scale = det.abs().sqrt();
                    let rot = m[1][0].atan2(m[0][0]);
                    // Quarter turns at most: a whole circle as one edge
                    // (one vertex) breaks the kernel's loft.
                    let k =
                        ((sweep.abs() / std::f64::consts::FRAC_PI_2 - 1e-9).ceil() as usize).max(1);
                    for i in 0..k {
                        out.push(sg::Seg2::Arc {
                            center: f(center),
                            radius: radius * scale,
                            start: start + rot + sweep * i as Real / k as Real,
                            sweep: sweep / k as Real,
                        });
                    }
                } else {
                    for c in arc_conics(center, radius, start, sweep) {
                        if let sg::Seg2::Conic { a, apex, b, w } = c {
                            out.push(sg::Seg2::Conic {
                                a: f(a),
                                apex: f(apex),
                                b: f(b),
                                w,
                            });
                        }
                    }
                }
            }
            sg::Seg2::Cubic { p0, p1, p2, p3 } => out.push(sg::Seg2::Cubic {
                p0: f(p0),
                p1: f(p1),
                p2: f(p2),
                p3: f(p3),
            }),
            sg::Seg2::Conic { a, apex, b, w } => out.push(sg::Seg2::Conic {
                a: f(a),
                apex: f(apex),
                b: f(b),
                w,
            }),
            sg::Seg2::Bezier { n, p } => {
                let mut q = p;
                for x in q.iter_mut().take(n as usize + 1) {
                    *x = f(*x);
                }
                out.push(sg::Seg2::Bezier { n, p: q });
            }
        }
        counts.push(out.len() - before);
    }
    (out, counts)
}

fn expand(names: &[String], counts: &[usize]) -> Vec<String> {
    names
        .iter()
        .zip(counts)
        .flat_map(|(n, &c)| std::iter::repeat_n(n.clone(), c))
        .collect()
}

/// A section from a profile placed by `frame` (`similar`: every frame of the
/// loft is a rotation with uniform scale, so arcs stay arcs).
fn placed_section(prof: &ExactProfile, frame: &Frame, similar: bool) -> Result<Section, String> {
    let lx = frame.x.norm();
    if lx < 1e-12 {
        return Err("a section shrinks to nothing".into());
    }
    let ex = frame.x / lx;
    let yp = frame.y - ex * frame.y.dot(&ex);
    let ly = yp.norm();
    if ly < 1e-12 {
        return Err("a section shrinks to nothing".into());
    }
    let ey = yp / ly;
    // (u, v) → (a, b) in the orthonormal frame (ex, ey).
    let m = [[lx, frame.y.dot(&ex)], [0.0, ly]];
    let plane = sg::Plane::new(v3(&frame.origin), d3(&ex), d3(&ey)).ok_or("degenerate section")?;
    let (on, hn) = split_names(prof);
    let zero = sg::Vec2::new(0.0, 0.0);
    let (outer, counts) = map_loop(&prof.region.outer, m, zero, similar);
    let holes = prof
        .region
        .holes
        .iter()
        .zip(&hn)
        .map(|(h, names)| {
            let (segs, c) = map_loop(h, m, zero, similar);
            (sg::Loop2 { segs }, expand(names, &c))
        })
        .collect();
    Ok(Section {
        plane,
        outer: sg::Loop2 { segs: outer },
        outer_names: expand(&on, &counts),
        holes,
    })
}

/// Whether a frame is a rotation with uniform scale.
fn is_similar(f: &Frame) -> bool {
    let (a, b) = (f.x.norm(), f.y.norm());
    (a - b).abs() <= 1e-9 * a.max(b).max(1.0) && f.x.dot(&f.y).abs() <= 1e-9 * a * b
}

/// Lofts copies of `prof` placed by `frames` (two or more, in order; a hole
/// of the profile is lofted through the same frames and cut out). Faces as
/// for [`sweep`].
pub fn loft_frames(
    prof: &ExactProfile,
    frames: &[Frame],
    prefix: &str,
) -> Result<ExactSolid, String> {
    if frames.len() < 2 {
        return Err("a loft needs two sections or more".into());
    }
    let similar = frames.iter().all(is_similar);
    let secs = frames
        .iter()
        .map(|f| placed_section(prof, f, similar))
        .collect::<Result<Vec<_>, _>>()?;
    loft_sections(&secs, prefix, false)
}

/// Lofts profiles on their own planes, in order (a hole needs a hole in
/// every section; holes are paired with the first section's nearest one).
pub fn loft(sections: &[(Plane, &ExactProfile)], prefix: &str) -> Result<ExactSolid, String> {
    if sections.len() < 2 {
        return Err("a loft needs two sections or more".into());
    }
    let secs = sections
        .iter()
        .map(|(pl, prof)| {
            let frame = Frame {
                origin: pl.origin,
                x: pl.x_axis.into_inner(),
                y: pl.y_axis().into_inner(),
            };
            placed_section(prof, &frame, true)
        })
        .collect::<Result<Vec<_>, String>>()?;
    loft_sections(&secs, prefix, true)
}

fn loft_sections(secs: &[Section], prefix: &str, rename_later: bool) -> Result<ExactSolid, String> {
    let holes = secs.first().map_or(0, |s| s.holes.len());
    if secs.iter().any(|s| s.holes.len() != holes) {
        return Err("every section needs the same number of holes".into());
    }
    // Names numbered through the sections in order (the kernel's numbering).
    let numbered = |pick: &dyn Fn(&Section) -> &[String]| -> Vec<String> {
        secs.iter()
            .enumerate()
            .flat_map(|(k, s)| {
                pick(s).iter().map(move |n| {
                    if k == 0 || !rename_later {
                        n.clone()
                    } else {
                        format!("s{k}.{n}")
                    }
                })
            })
            .collect()
    };
    let outer_names = numbered(&|s| &s.outer_names);
    let input: Vec<(sg::Plane, sg::Loop2)> =
        secs.iter().map(|s| (s.plane, s.outer.clone())).collect();
    let body = guarded("loft", || sk::loft(&input))?;
    let face_names = names_from(&body, &[], &side_names(&outer_names, prefix));
    let mut cur = ExactSolid {
        sheet: false,
        name: Some(prefix.to_owned()),
        body,
        face_names,
        profiles: Vec::new(),
    };
    // Each hole: the first section's hole paired with the nearest hole of
    // every other section (by centroid in model space), lofted and cut.
    for h in 0..holes {
        let c0 = secs[0].plane.to_world(secs[0].holes[h].0.centroid());
        let mut input = Vec::new();
        let mut names = Vec::new();
        for (k, s) in secs.iter().enumerate() {
            let j = if k == 0 {
                h
            } else {
                (0..s.holes.len())
                    .min_by(|&a, &b| {
                        let da = s.plane.to_world(s.holes[a].0.centroid()).dist(c0);
                        let db = s.plane.to_world(s.holes[b].0.centroid()).dist(c0);
                        da.total_cmp(&db)
                    })
                    .unwrap_or(h)
            };
            let (lp, n) = &s.holes[j];
            input.push((s.plane, lp.clone()));
            names.extend(n.iter().map(|x| {
                if k == 0 || !rename_later {
                    x.clone()
                } else {
                    format!("s{k}.{x}")
                }
            }));
        }
        let tool = guarded("loft", || sk::loft(&input))?;
        let face_names = names_from(&tool, &[], &side_names(&names, prefix));
        let tool = ExactSolid {
            sheet: false,
            name: None,
            body: tool,
            face_names,
            profiles: Vec::new(),
        };
        cur = super::boolean(&cur, &tool, super::ExactOp::Cut)?
            .ok_or("the hole fills the whole section")?;
    }
    Ok(cur)
}

/// Lofts the outer loop of `prof` (straight segments only) to the point
/// `apex` off its plane (a pyramid; Inventor's Sharp Point).
pub fn loft_to_point(
    plane: &Plane,
    prof: &ExactProfile,
    apex: Point3,
    prefix: &str,
) -> Result<ExactSolid, String> {
    if !prof.region.holes.is_empty() {
        return Err("a section lofted to a point cannot have holes".into());
    }
    let pl = plane_of(plane)?;
    let body = guarded("loft", || {
        sk::loft_to_point(&pl, &prof.region.outer, v3(&apex))
    })?;
    named(body, plane, &prof.names, prefix)
}

/// The world points of a profile's outer loop placed by `frame` (for
/// previews and tests).
pub fn frame_point(frame: &Frame, u: Real, v: Real) -> Point3 {
    frame.origin + frame.x * u + frame.y * v
}
