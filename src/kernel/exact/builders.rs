//! Bodies our library and frame commands build from a description, on the solidcore kernel: a
//! stepped body about Z (a fastener's meridian revolved, flats cut by a ring), pipe bends and tees,
//! a prism between two planes (a frame member). Each returns the body with a number per face (the
//! caller's role for it), read from the kernel's history, never from an index of its own.

use std::f64::consts::TAU;

use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use solidcraft_geom::{
    Loop2Geom as _, PlaneGeom as _, Seg2Geom as _, Vec2Geom as _, Vec3Geom as _,
};
use solidcraft_kernel as k;
use solidcraft_kernel::history::FaceOrigin;
use solidcraft_kernel::{Body, BoolOp, KernelError, Result};

use super::compat::sk::{AxialDrill, AxialRun, AxialShape};

/// A role per face.
pub type Numbers = Vec<Option<usize>>;

fn invalid<T>(m: &str) -> Result<T> {
    Err(KernelError::Invalid(m.to_owned()))
}

/// Each face of a swept body: its role from what it was swept from.
fn swept(b: &Body, role: impl Fn(&FaceOrigin) -> Option<usize>) -> Numbers {
    match b.history() {
        Some(h) => {
            let mut out: Numbers = h
                .faces
                .iter()
                .map(|f| f.origins.iter().find_map(&role))
                .collect();
            out.resize(b.face_count(), None);
            out
        }
        None => vec![None; b.face_count()],
    }
}

/// Each face of a Boolean's result: the role of the operand's face it came from.
fn carried(b: &Body, operands: [&Numbers; 2]) -> Numbers {
    swept(b, |o| match o {
        FaceOrigin::Face { operand, face } => operands
            .get(*operand)
            .and_then(|n| n.get(*face))
            .copied()
            .flatten(),
        _ => None,
    })
}

fn boolean(a: (&Body, &Numbers), b: (&Body, &Numbers), op: BoolOp) -> Result<(Body, Numbers)> {
    let Some(r) = k::boolean(a.0, b.0, op)? else {
        return invalid("nothing is left of the part");
    };
    let n = carried(&r, [a.1, b.1]);
    Ok((r, n))
}

fn one(mut bodies: Vec<Body>) -> Result<Body> {
    match bodies.len() {
        1 => bodies
            .pop()
            .ok_or_else(|| KernelError::Invalid("no body".into())),
        _ => invalid("the profile did not give one body"),
    }
}

/// A plane through `origin`, square to `x` × `y`.
fn plane(origin: Vec3, x: Vec3, y: Vec3) -> Plane {
    Plane { origin, x, y }
}

fn line(a: Vec2, b: Vec2) -> Seg2 {
    Seg2::Line { a, b }
}

fn clockwise_circle(center: Vec2, radius: f64) -> Loop2 {
    Loop2 {
        segs: vec![Seg2::Arc {
            center,
            radius,
            start: 0.0,
            sweep: -TAU,
        }],
    }
}

/// Whether a prism run walked from `z0` to `z1` along a chain of this sense is a pocket in the
/// material (a socket), not a head.
fn prism_is_inside(z0: f64, z1: f64, ccw: bool) -> bool {
    (z1 > z0) != ccw
}

/// The meridian of `runs` in (radius, z): a closed chain with the role of each segment. A prism
/// run stands as a wall outside its corners; [`axial_stack`] cuts its flats out of it.
fn meridian(
    runs: &[AxialRun],
    first_cap: Option<usize>,
    closed: bool,
    ccw: bool,
) -> Result<Vec<(Seg2, Option<usize>)>> {
    let mut chain: Vec<(Seg2, Option<usize>)> = Vec::new();
    fn push(chain: &mut Vec<(Seg2, Option<usize>)>, a: Vec2, b: Vec2, n: Option<usize>) {
        if a.dist(b) > 1e-9 {
            chain.push((line(a, b), n));
        }
    }
    let mut first: Option<Vec2> = None;
    let mut last: Option<(Vec2, Option<usize>)> = None;
    for run in runs {
        let (start, end, segs): (Vec2, Vec2, Vec<(Seg2, Option<usize>)>) = match &run.shape {
            AxialShape::Turned { segs, numbers } => {
                let (Some(a), Some(b)) = (segs.first(), segs.last()) else {
                    return invalid("a turned run without segments");
                };
                let list = segs
                    .iter()
                    .enumerate()
                    .map(|(i, s)| (*s, numbers.get(i).copied()))
                    .collect();
                (a.start(), b.end(), list)
            }
            AxialShape::Prism {
                corners,
                z0,
                z1,
                start_cone,
                end_cone,
                ..
            } => {
                let rc = corners.iter().map(|c| c.len()).fold(0.0, f64::max);
                let d = (z1 - z0).signum();
                // Walking up a counter-clockwise chain the material is towards the axis (a head);
                // walking down it is outside (a socket), and the wall stands inside the flats.
                let wall = if prism_is_inside(*z0, *z1, ccw) {
                    rc * (TAU / 2.0 / corners.len() as f64).cos() * 0.9
                } else {
                    rc * 1.05
                };
                let mut list = Vec::new();
                let (mut a, mut w0) = (Vec2::new(wall, *z0), None);
                if let Some(c) = start_cone {
                    a = Vec2::new(c.radius, *z0);
                    let p = Vec2::new(wall, z0 + d * c.slope * (wall - c.radius).abs());
                    push(&mut list, a, p, Some(c.number));
                    w0 = Some(p);
                }
                let (mut b, mut w1) = (Vec2::new(wall, *z1), None);
                if let Some(c) = end_cone {
                    b = Vec2::new(c.radius, *z1);
                    w1 = Some((
                        Vec2::new(wall, z1 - d * c.slope * (wall - c.radius).abs()),
                        c.number,
                    ));
                }
                push(&mut list, w0.unwrap_or(a), w1.map_or(b, |w| w.0), None);
                if let Some((p, n)) = w1 {
                    push(&mut list, p, b, Some(n));
                }
                (a, b, list)
            }
        };
        if let Some((p, n)) = last {
            push(&mut chain, p, start, n);
        }
        first.get_or_insert(start);
        chain.extend(segs);
        last = Some((end, run.after));
    }
    let (Some(first), Some((end, after))) = (first, last) else {
        return invalid("a stepped body without runs");
    };
    if closed {
        push(&mut chain, end, first, after);
    } else {
        let (top, bottom) = (Vec2::new(0.0, end.y), Vec2::new(0.0, first.y));
        push(&mut chain, end, top, after);
        push(&mut chain, top, bottom, None);
        push(&mut chain, bottom, first, first_cap);
    }
    Ok(chain)
}

/// The body of `runs` about Z, with the role of each face.
pub fn axial_stack(
    runs: &[AxialRun],
    first_cap: Option<usize>,
    closed: bool,
    drills: &[AxialDrill],
) -> Result<(Body, Numbers)> {
    // The sense comes from a first walk; it decides which side of each prism its wall stands on.
    let trial = meridian(runs, first_cap, closed, true)?;
    let ccw = Loop2 {
        segs: trial.iter().map(|c| c.0).collect(),
    }
    .signed_area()
        > 0.0;
    let mut chain = meridian(runs, first_cap, closed, ccw)?;
    let mut outer = Loop2 {
        segs: chain.iter().map(|c| c.0).collect(),
    };
    if !ccw {
        outer = outer.ccw();
        chain.reverse();
    }
    let xz = plane(Vec3::ZERO, Vec3::X, Vec3::Z);
    let region = Region2 {
        outer,
        holes: Vec::new(),
    };
    let body = one(k::revolve(
        &xz,
        &[region],
        Vec2::ZERO,
        Vec2::new(0.0, 1.0),
        TAU,
    )?)?;
    let roles = swept(&body, |o| match o {
        FaceOrigin::Profile { segment } => chain.get(*segment).and_then(|c| c.1),
        _ => None,
    });
    let mut part = (body, roles);
    let mut before = first_cap;
    for run in runs {
        let (below, above) = (before, run.after);
        before = run.after;
        let AxialShape::Prism {
            corners,
            z0,
            z1,
            numbers,
            ..
        } = &run.shape
        else {
            continue;
        };
        let n = corners.len();
        if n < 3 || numbers.len() < n {
            return invalid("a prism run needs a role per side");
        }
        let rc = corners.iter().map(|c| c.len()).fold(0.0, f64::max);
        let inside = prism_is_inside(*z0, *z1, ccw);
        let (zl, zh) = (z0.min(*z1), z0.max(*z1));
        // A head standing on a planar step wider than it (a flange): a ring ending flush on that
        // step leaves SMLib a result its check refuses, so what stays is intersected instead.
        let wall = rc * 1.05;
        let steps_at = |z: f64| {
            chain.iter().any(|c| {
                let (a, b) = (c.0.start(), c.0.end());
                (a.y - z).abs() < 1e-9
                    && (b.y - z).abs() < 1e-9
                    && a.x.max(b.x) > wall * (1.0 + 1e-9)
            })
        };
        if !inside && (steps_at(zl) || steps_at(zh)) {
            let (low, high) = if z0 < z1 {
                (below, above)
            } else {
                (above, below)
            };
            let segs: Vec<Seg2> = (0..n)
                .map(|i| line(corners[i], corners[(i + 1) % n]))
                .collect();
            let head = one(k::extrude(
                &Plane::XY,
                &[Region2 {
                    outer: Loop2 { segs },
                    holes: Vec::new(),
                }],
                zl,
                zh,
            )?)?;
            let head_roles = swept(&head, |o| match o {
                FaceOrigin::Profile { segment } => numbers.get(*segment).copied(),
                FaceOrigin::Cap { end } => {
                    if *end {
                        high
                    } else {
                        low
                    }
                }
                _ => None,
            });
            let points: Vec<Vec2> = chain
                .iter()
                .flat_map(|c| [c.0.start(), c.0.end()])
                .collect();
            let reach = points.iter().map(|p| p.x).fold(rc, f64::max) * 1.2;
            let bottom = points.iter().map(|p| p.y).fold(zl, f64::min);
            let top = points.iter().map(|p| p.y).fold(zh, f64::max);
            let mut keep = (head, head_roles);
            for (from, to) in [(bottom - 1.0, zl), (zh, top + 1.0)] {
                if (from < bottom && bottom < zl) || (to > top && top > zh) {
                    let disc = one(k::extrude(
                        &Plane::XY,
                        &[Region2 {
                            outer: Loop2::circle(Vec2::ZERO, reach),
                            holes: Vec::new(),
                        }],
                        from,
                        to,
                    )?)?;
                    let none = vec![None; disc.face_count()];
                    keep = boolean((&keep.0, &keep.1), (&disc, &none), BoolOp::Union)?;
                }
            }
            part = boolean((&part.0, &part.1), (&keep.0, &keep.1), BoolOp::Intersect)?;
            continue;
        }
        // What comes off the wall: outside a head a ring with the polygon as its hole; in a socket
        // the polygon with a hole inside the wall.
        let (ring, sides, skip) = if inside {
            let flats = rc * (TAU / 2.0 / n as f64).cos();
            let segs: Vec<Seg2> = (0..n)
                .map(|i| line(corners[i], corners[(i + 1) % n]))
                .collect();
            (
                Region2 {
                    outer: Loop2 { segs },
                    holes: vec![clockwise_circle(Vec2::ZERO, flats * 0.8)],
                },
                numbers[..n].to_vec(),
                0,
            )
        } else {
            let hole: Vec<Seg2> = (0..n)
                .rev()
                .map(|i| line(corners[(i + 1) % n], corners[i]))
                .collect();
            let round = Loop2::circle(Vec2::ZERO, rc * 1.2);
            let skip = round.segs.len();
            (
                Region2 {
                    outer: round,
                    holes: vec![Loop2 { segs: hole }],
                },
                (0..n).rev().map(|i| numbers[i]).collect(),
                skip,
            )
        };
        let tool = one(k::extrude(&Plane::XY, &[ring], z0.min(*z1), z0.max(*z1))?)?;
        // A cap lies on the planar face at that end of the run.
        let (low, high) = if z0 < z1 {
            (below, above)
        } else {
            (above, below)
        };
        let tool_roles = swept(&tool, |o| match o {
            FaceOrigin::Profile { segment } => segment
                .checked_sub(skip)
                .and_then(|s| sides.get(s))
                .copied(),
            FaceOrigin::Cap { end } => {
                if *end {
                    high
                } else {
                    low
                }
            }
            _ => None,
        });
        part = boolean((&part.0, &part.1), (&tool, &tool_roles), BoolOp::Cut)?;
    }
    for d in drills {
        let disc = Region2 {
            outer: Loop2::circle(d.center, d.radius),
            holes: Vec::new(),
        };
        let tool = one(k::extrude(
            &Plane::XY,
            &[disc],
            d.z0.min(d.z1),
            d.z0.max(d.z1),
        )?)?;
        let tool_roles = swept(&tool, |o| {
            matches!(o, FaceOrigin::Profile { .. }).then_some(d.number)
        });
        part = boolean((&part.0, &part.1), (&tool, &tool_roles), BoolOp::Cut)?;
    }
    Ok(part)
}

/// A bend of pipe: the end on the XY plane about the origin, leaving along +Z and turning towards
/// +X about a line at `bend` from the axis. Roles: outside, bore, the end on the plane, the far end.
pub fn pipe_bend(
    radius: f64,
    bore: f64,
    bend: f64,
    angle: f64,
    numbers: [usize; 4],
) -> Result<(Body, Numbers)> {
    if !(bore > 0.0 && bore < radius && radius < bend && angle > 0.0 && angle < TAU / 2.0) {
        return invalid(
            "a bend needs a bore inside its radius, a bend beyond it and an angle below a half turn",
        );
    }
    let outer = Loop2::circle(Vec2::ZERO, radius);
    let skip = outer.segs.len();
    let ring = Region2 {
        outer,
        holes: vec![clockwise_circle(Vec2::ZERO, bore)],
    };
    let axis = (Vec2::new(bend, 0.0), Vec2::new(0.0, 1.0));
    let mut body = one(k::revolve(
        &Plane::XY,
        std::slice::from_ref(&ring),
        axis.0,
        axis.1,
        angle,
    )?)?;
    if body.exact_bounds()?.max.z <= 1e-6 {
        body = one(k::revolve(
            &Plane::XY,
            std::slice::from_ref(&ring),
            axis.0,
            axis.1,
            -angle,
        )?)?;
    }
    let roles = swept(&body, |o| match o {
        FaceOrigin::Profile { segment } => Some(numbers[usize::from(*segment >= skip)]),
        FaceOrigin::Cap { end } => Some(numbers[2 + usize::from(*end)]),
        _ => None,
    });
    Ok((body, roles))
}

/// A straight tee of pipe: the run along +Z from the XY plane with its middle at `c`, the branch from
/// there along +X, each end `c` from the middle. Roles in `numbers`: run, branch, run bore, branch
/// bore, low end, high end, branch end.
pub fn pipe_tee(radius: f64, bore: f64, c: f64, numbers: [usize; 7]) -> Result<(Body, Numbers)> {
    if !(bore > 0.0 && bore < radius && radius < c) {
        return invalid("a tee needs a bore inside its radius and ends beyond it");
    }
    let [run, branch, run_bore, branch_bore, low, high, branch_end] = numbers;
    let side = plane(Vec3::new(0.0, 0.0, c), Vec3::Y, Vec3::Z);
    let disc = |r: f64| Region2 {
        outer: Loop2::circle(Vec2::ZERO, r),
        holes: Vec::new(),
    };
    let a = one(k::extrude(&Plane::XY, &[disc(radius)], 0.0, 2.0 * c)?)?;
    let a_roles = swept(&a, |o| match o {
        FaceOrigin::Profile { .. } => Some(run),
        FaceOrigin::Cap { end } => Some(if *end { high } else { low }),
        _ => None,
    });
    let b = one(k::extrude(&side, &[disc(radius)], 0.0, c)?)?;
    let b_roles = swept(&b, |o| match o {
        FaceOrigin::Profile { .. } => Some(branch),
        FaceOrigin::Cap { end: true } => Some(branch_end),
        _ => None,
    });
    let outside = boolean((&a, &a_roles), (&b, &b_roles), BoolOp::Union)?;
    let over = c * 0.5;
    let p = one(k::extrude(
        &Plane::XY,
        &[disc(bore)],
        -over,
        2.0 * c + over,
    )?)?;
    let p_roles = swept(&p, |o| {
        matches!(o, FaceOrigin::Profile { .. }).then_some(run_bore)
    });
    let q = one(k::extrude(&side, &[disc(bore)], 0.0, c + over)?)?;
    let q_roles = swept(&q, |o| {
        matches!(o, FaceOrigin::Profile { .. }).then_some(branch_bore)
    });
    let cut = boolean((&outside.0, &outside.1), (&p, &p_roles), BoolOp::Cut)?;
    boolean((&cut.0, &cut.1), (&q, &q_roles), BoolOp::Cut)
}

/// A prism between two planes: each loop's section on the start and on the end plane (segments
/// matched, outer first). Roles: side `k` counts segments across the loops, then the start and
/// the end face.
pub fn prism_between(loops: &[[(Plane, Loop2); 2]]) -> Result<(Body, Numbers)> {
    let Some([(p0, l0), (p1, l1)]) = loops.first() else {
        return invalid("a prism without a section");
    };
    let (Some(s0), Some(s1)) = (l0.segs.first(), l1.segs.first()) else {
        return invalid("a prism with an empty section");
    };
    let (a, b) = (p0.to_world(s0.start()), p1.to_world(s1.start()));
    let Some(dir) = (b - a).normalized() else {
        return invalid("a prism without a length");
    };
    // Sections that are not one shape moved along one direction (a drafted wall): a ruled solid
    // between them. Pushing the start section along would leave the walls straight.
    let tol = 1e-7 * (b - a).len().max(1.0);
    let moved = loops.iter().all(|[(q0, m0), (q1, m1)]| {
        m0.segs.len() == m1.segs.len()
            && m0.segs.iter().zip(&m1.segs).all(|(s, e)| {
                let step = q1.to_world(e.start()) - q0.to_world(s.start());
                // Along the prism's direction, whatever the end planes' tilt.
                step.cross(dir).len() <= tol
            })
    });
    if !moved {
        let region = |i: usize| {
            let mut all = loops.iter().map(|l| l[i].1.clone());
            all.next().map(|outer| Region2 {
                outer,
                holes: all.collect(),
            })
        };
        let (Some(r0), Some(r1)) = (region(0), region(1)) else {
            return invalid("a prism without a section");
        };
        let body = k::loft_regions(&[(*p0, r0), (*p1, r1)])?;
        let count: usize = loops.iter().map(|l| l[0].1.segs.len()).sum();
        let roles = swept(&body, |o| match o {
            FaceOrigin::Profile { segment } => Some(*segment),
            FaceOrigin::Cap { end } => Some(if *end { count + 1 } else { count }),
            _ => None,
        });
        return Ok((body, roles));
    }
    let helper = if dir.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
    let Some(x) = dir.cross(helper).normalized() else {
        return invalid("a prism without a direction");
    };
    let square = plane(a, x, dir.cross(x));
    // A section square to the direction: its points are the start section's along it.
    let local = |p: Vec2| square.to_local(p0.to_world(p));
    let project = |s: &Seg2| match *s {
        Seg2::Conic { a, apex, b, w } => Ok(Seg2::Conic {
            a: local(a),
            apex: local(apex),
            b: local(b),
            w,
        }),
        Seg2::Line { a, b } => Ok(line(local(a), local(b))),
        _ => invalid("a prism's section is lines and conics"),
    };
    let section = |l: &Loop2| -> Result<Loop2> {
        Ok(Loop2 {
            segs: l.segs.iter().map(project).collect::<Result<_>>()?,
        })
    };
    let mut sections = loops.iter().map(|l| section(&l[0].1));
    let Some(outer) = sections.next() else {
        return invalid("a prism without a section");
    };
    let region = Region2 {
        outer: outer?,
        holes: sections.collect::<Result<_>>()?,
    };
    let count: usize = loops.iter().map(|l| l[0].1.segs.len()).sum();
    let (start_role, end_role) = (count, count + 1);
    let mut heights = Vec::new();
    for l in loops {
        for s in &l[0].1.segs {
            heights.push(square.height(p0.to_world(s.start())));
        }
        for s in &l[1].1.segs {
            heights.push(square.height(p1.to_world(s.start())));
        }
    }
    let lo = heights.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = heights.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let flat = |p: &Plane| p.normal().cross(dir).len() < 1e-9;
    let swept_role = |o: &FaceOrigin| match o {
        FaceOrigin::Profile { segment } => Some(*segment),
        FaceOrigin::Cap { end } => Some(if *end { end_role } else { start_role }),
        _ => None,
    };
    if flat(p0) && flat(p1) {
        let body = one(k::extrude(&square, &[region], 0.0, square.height(b))?)?;
        let roles = swept(&body, swept_role);
        return Ok((body, roles));
    }
    let margin = (hi - lo).max(1.0);
    let long = one(k::extrude(&square, &[region], lo - margin, hi + margin)?)?;
    let long_roles = swept(&long, |o| match o {
        FaceOrigin::Profile { segment } => Some(*segment),
        _ => None,
    });
    let middle = a + dir * (square.height(b) * 0.5);
    let mut part = (long, long_roles);
    for (p, role) in [(p0, start_role), (p1, end_role)] {
        let positive = p.height(middle) > 0.0;
        let (kept, _tool) = k::half_space(&part.0, p, positive)?;
        let Some(kept) = kept else {
            return invalid("nothing is left of the prism between its planes");
        };
        let roles = swept(&kept, |o| match o {
            FaceOrigin::Face { operand: 0, face } => part.1.get(*face).copied().flatten(),
            FaceOrigin::Face { operand: 1, .. } => Some(role),
            _ => None,
        });
        part = (kept, roles);
    }
    Ok(part)
}

/// A clamped cubic B-spline sheet whose control net is `net`.
pub fn bspline_sheet(net: &[Vec<Vec3>], wrap: bool) -> Result<Body> {
    let n = net.len();
    let nv = net.first().map_or(0, Vec::len);
    if n < 2 || nv < 2 || net.iter().any(|r| r.len() != nv) || n * nv > 100_000 {
        return invalid("a control net needs equal rows of at least two points");
    }
    let open = |count: usize| -> Vec<f64> {
        let degree = 3.min(count - 1);
        let mut knots = vec![0.0; degree + 1];
        for i in 1..count.saturating_sub(degree) {
            knots.push(i as f64);
        }
        knots.extend(vec![count.saturating_sub(degree) as f64; degree + 1]);
        knots
    };
    let degree_u = 3.min(nv - 1);
    let (degree_v, knots_v, rows): (usize, Vec<f64>, Vec<&Vec<Vec3>>) = if wrap {
        if n < 3 {
            return invalid("a closed net needs three rings");
        }
        let rows = net.iter().chain(net.iter().take(3)).collect();
        (3, (0..n + 3 + 4).map(|i| i as f64).collect(), rows)
    } else {
        (3.min(n - 1), open(n), net.iter().collect())
    };
    let points: Vec<Vec3> = rows.iter().flat_map(|r| r.iter().copied()).collect();
    let weights = vec![1.0; points.len()];
    let fail = |e| KernelError::Invalid(format!("control net: {e}"));
    let surface = solidcraft_geom::bspline::NurbsSurface::new(
        [degree_u, degree_v],
        [open(nv), knots_v],
        &points,
        &weights,
    )
    .map_err(fail)?;
    let surface = if wrap {
        surface.clamped().map_err(fail)?
    } else {
        surface
    };
    // Its normal as the program's surfaces have always had it (Thicken's side reads it).
    k::reverse_faces(&k::tensor_sheet(&surface)?, &[])
}
