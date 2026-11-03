//! 3DCadCraft addition: a stepped body about the Z axis built face by face, without a Boolean
//! (a bolt's hexagon joined to its shank took the general Boolean ~240 ms, this ~5 ms).
//!
//! The body is walked along its meridian as a chain of runs: a turned chain of lines and arcs
//! in (radius, z), or a polygon prism (a hexagon head, a socket). Neighbouring runs meet in a
//! planar face at their common height; the two rings of that face keep the very edges of the
//! runs, so the shell closes without any intersection work. A prism's ends may be cut by a
//! cone (a hexagon's 30° corner chamfer): the cone meets each flat in an exact hyperbola.

use std::collections::HashMap;

use mt::builder;
use solidcraft_geom::{Plane, Seg2, Vec2, Vec3};
use truck_modeling as mt;
use truck_modeling::{BoundedCurve, ParametricCurve};

use crate::body::{Body, Solid, from_p3, p3, v3};
use crate::history::FaceProv;
use crate::record;
use crate::{KernelError, Result, guard};

/// A cone cutting a prism's end: the circle it leaves on the end plane and the slope of its
/// generator (height lost per unit of radius).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AxialCone {
    /// Radius of the circle on the end plane, inside the polygon's flats.
    pub radius: f64,
    /// Height lost per unit of radius.
    pub slope: f64,
    /// Profile number of the cone's faces.
    pub number: usize,
}

/// One run of the meridian.
#[derive(Clone, Debug, PartialEq)]
pub enum AxialShape {
    /// Lines and arcs in (radius, z), revolved fully about Z, each with its profile number. An
    /// end on the axis is a tip; any other end is a ring.
    Turned {
        /// The chain, end to end.
        segs: Vec<Seg2>,
        /// Profile number per segment.
        numbers: Vec<usize>,
    },
    /// A polygon about Z from height `z0` to `z1` (either way).
    Prism {
        /// Its corners, counter-clockwise about +Z, on one circle about the axis.
        corners: Vec<Vec2>,
        /// Height the run starts at.
        z0: f64,
        /// Height the run ends at.
        z1: f64,
        /// Profile number per side (side `i` starts at corner `i`).
        numbers: Vec<usize>,
        /// Cone cutting the `z0` end.
        start_cone: Option<AxialCone>,
        /// Cone cutting the `z1` end.
        end_cone: Option<AxialCone>,
    },
}

/// A run and the planar face after it (towards the next run, or the end's cap).
#[derive(Clone, Debug, PartialEq)]
pub struct AxialRun {
    /// Its shape.
    pub shape: AxialShape,
    /// Profile number of the planar face after it; `None` only where a tip ends the chain.
    pub after: Option<usize>,
}

/// A round hole straight along Z between two of the body's planar faces, parallel to the axis
/// and off it (a flange's bolt hole): one cylinder, no Boolean.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AxialDrill {
    /// Its centre in the planes.
    pub center: Vec2,
    /// Its radius.
    pub radius: f64,
    /// Height of one end.
    pub z0: f64,
    /// Height of the other end.
    pub z1: f64,
    /// Profile number of its wall.
    pub number: usize,
}

const TAU: f64 = std::f64::consts::TAU;

/// A built run: its faces and the rings at its two ends (`None`: a tip).
struct Built {
    faces: Vec<(mt::Face, FaceProv)>,
    start: Option<mt::Wire>,
    end: Option<mt::Wire>,
}

/// The body of `runs`. `first_cap`: profile number of the disc closing the first run's start,
/// `None` when that start is a tip or the chain is `closed` (the last run's face then joins its
/// end to the first run's start).
pub fn axial_stack(runs: &[AxialRun], first_cap: Option<usize>, closed: bool) -> Result<Body> {
    axial_stack_drilled(runs, first_cap, closed, &[])
}

/// The same with round holes between its planar faces. Each hole's rings join the planar face
/// at each of its ends; a hole with no face there is refused.
pub fn axial_stack_drilled(runs: &[AxialRun], first_cap: Option<usize>, closed: bool, drills: &[AxialDrill]) -> Result<Body> {
    if runs.is_empty() {
        return Err(KernelError::Invalid("a stepped body needs a run".into()));
    }
    guard("stepped body", || {
        let built = runs.iter().map(|r| build_run(&r.shape)).collect::<Result<Vec<_>>>()?;
        let mut faces: Vec<(mt::Face, FaceProv)> = Vec::new();
        for b in &built {
            faces.extend(b.faces.iter().cloned());
        }
        let mut holes = Vec::with_capacity(drills.len());
        for d in drills {
            let (walls, low, high) = drill(d)?;
            faces.extend(walls);
            holes.push((d.z0.min(d.z1), low, d.z0.max(d.z1), high, *d, [false, false]));
        }
        // The rings of the holes that end on the planar face at height `z` between these radii.
        let mut holes_on = |z: f64, inner: f64, outer: f64| -> Vec<mt::Wire> {
            let mut out = Vec::new();
            for (low_z, low, high_z, high, d, used) in &mut holes {
                let c = d.center.len();
                if c - d.radius <= inner || c + d.radius >= outer {
                    continue;
                }
                if (*low_z - z).abs() < 1e-9 {
                    out.push(low.clone());
                    used[0] = true;
                } else if (*high_z - z).abs() < 1e-9 {
                    out.push(high.clone());
                    used[1] = true;
                }
            }
            out
        };
        if let (Some(n), false) = (first_cap, closed) {
            let ring = built.first().and_then(|b| b.start.clone()).ok_or_else(|| fail("the first run has no ring to close"))?;
            let extra = holes_on(ring_height(&ring), 0.0, ring_measure(&ring).0);
            faces.push((with_holes(&ring, None, &extra)?, record::profile(&[n])));
        }
        for (k, r) in runs.iter().enumerate() {
            let ring = built.get(k).and_then(|b| b.end.clone());
            let next = if k + 1 < runs.len() {
                built.get(k + 1).and_then(|b| b.start.clone())
            } else if closed {
                built.first().and_then(|b| b.start.clone())
            } else {
                None
            };
            let number = r.after;
            match (ring, next, number) {
                (Some(a), Some(b), Some(n)) => {
                    let (ra, rb) = (ring_measure(&a).0, ring_measure(&b).0);
                    let extra = holes_on(ring_height(&a), ra.min(rb), ra.max(rb));
                    let (outer, inner) = if ra >= rb { (&a, &b) } else { (&b, &a) };
                    faces.push((with_holes(outer, Some(inner), &extra)?, record::profile(&[n])));
                }
                (Some(a), None, Some(n)) if k + 1 == runs.len() => {
                    let extra = holes_on(ring_height(&a), 0.0, ring_measure(&a).0);
                    faces.push((with_holes(&a, None, &extra)?, record::profile(&[n])));
                }
                (None, None, None) if k + 1 == runs.len() => {}
                _ => return Err(fail("the runs do not join")),
            }
        }
        if holes.iter().any(|h| h.5 != [true, true]) {
            return Err(fail("a hole does not end on planar faces"));
        }
        let (faces, history): (Vec<mt::Face>, Vec<FaceProv>) = oriented(faces)?.into_iter().unzip();
        let shell: mt::Shell = faces.into();
        let solid = Solid::try_new(vec![shell]).map_err(|e| fail(&format!("the shell does not close: {e}")))?;
        // Which way the shell turned is read off its lowest planar face, whose outside faces -Z:
        // meshing the body for its volume's sign was most of a domed nut's time.
        let body = match bottom_faces_out(&solid) {
            Some(out) => Body::new_placed(solid, !out, None, false)?,
            None => Body::new(solid)?,
        };
        record::attach(body, &[], Some(history), Default::default())
    })
}

/// Whether the solid's lowest face faces down; `None` when no planar face lies at its lowest
/// point (a tip at the bottom).
fn bottom_faces_out(solid: &Solid) -> Option<bool> {
    let low = |f: &mt::Face| -> f64 {
        f.absolute_boundaries().iter().flat_map(|w| w.vertex_iter()).map(|v| v.point().z).fold(f64::INFINITY, f64::min)
    };
    let lowest = solid.face_iter().map(low).fold(f64::INFINITY, f64::min);
    let face = solid.face_iter().find(|f| matches!(f.surface(), mt::Surface::Plane(_)) && (low(f) - lowest).abs() < 1e-9 && flat_at(f, lowest))?;
    match face.oriented_surface() {
        mt::Surface::Plane(p) => Some(p.normal().z < 0.0),
        _ => None,
    }
}

/// Every vertex of the face lies at height `z`.
fn flat_at(f: &mt::Face, z: f64) -> bool {
    f.absolute_boundaries().iter().flat_map(|w| w.vertex_iter()).all(|v| (v.point().z - z).abs() < 1e-9)
}

fn fail(m: &str) -> KernelError {
    KernelError::Failed(format!("stepped body: {m}"))
}

fn build_run(shape: &AxialShape) -> Result<Built> {
    match shape {
        AxialShape::Turned { segs, numbers } => turned(segs, numbers),
        AxialShape::Prism { corners, z0, z1, numbers, start_cone, end_cone } => match (start_cone, end_cone) {
            (None, None) => prism(corners, *z0, *z1, numbers),
            _ => coned_prism(corners, *z0, *z1, numbers, start_cone.as_ref(), end_cone.as_ref()),
        },
    }
}

/// The revolved chain. Built as `revolve_touching_axis` does: `cone` when an end is on the axis.
fn turned(segs: &[Seg2], numbers: &[usize]) -> Result<Built> {
    if segs.is_empty() || segs.len() != numbers.len() {
        return Err(fail("a turned run needs its segments and their numbers"));
    }
    // Meridian coordinates: x = radius, y = height.
    let plane = Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Z };
    let on_axis = |p: Vec2| p.x.abs() < 1e-9;
    let verts: Vec<mt::Vertex> = segs
        .iter()
        .map(|s| builder::vertex(p3(plane.to_world(s.start()))))
        .chain(segs.last().map(|s| builder::vertex(p3(plane.to_world(s.end())))))
        .collect();
    let mut edges: Vec<mt::Edge> = Vec::with_capacity(segs.len());
    for (i, s) in segs.iter().enumerate() {
        let (Some(a), Some(b)) = (verts.get(i), verts.get(i + 1)) else { continue };
        edges.push(match *s {
            Seg2::Line { .. } => builder::line(a, b),
            Seg2::Arc { .. } => builder::circle_arc(a, b, p3(plane.to_world(s.mid()))),
            _ => return Err(fail("a turned run takes lines and arcs")),
        });
    }
    let seeds: Vec<(mt::EdgeID, Vec<usize>)> = edges.iter().map(|e| e.id()).zip(numbers.iter().map(|n| vec![*n])).collect();
    let (first, last) = (segs.first().map(Seg2::start), segs.last().map(Seg2::end));
    let (apex_start, apex_end) = (first.is_some_and(on_axis), last.is_some_and(on_axis));
    if apex_start && apex_end {
        return Err(fail("a turned run with both ends on the axis"));
    }
    let mut w: mt::Wire = edges.into();
    let shell = if apex_start || apex_end {
        if apex_end {
            // `cone` wants the apex first.
            w = w.inverse();
        }
        builder::cone(&w, v3(Vec3::Z), mt::Rad(TAU))
    } else {
        builder::rsweep(&w, p3(Vec3::ZERO), v3(Vec3::Z), mt::Rad(TAU))
    };
    let fixed: Vec<mt::VertexID> =
        [(apex_start, verts.first()), (apex_end, verts.last())].into_iter().filter_map(|(apex, v)| v.filter(|_| apex).map(|v| v.id())).collect();
    let provenance: Vec<FaceProv> = record::columns(shell.face_iter(), &seeds, None, record::Column::ToEnd, &fixed)
        .and_then(|sides| sides.into_iter().collect())
        .ok_or_else(|| fail("a turned face lost its segment"))?;
    let faces: Vec<(mt::Face, FaceProv)> = shell.face_iter().cloned().zip(provenance).collect();
    let boundaries = shell.extract_boundaries();
    let ring_at = |v: Option<&mt::Vertex>| {
        let id = v.map(|v| v.id());
        boundaries.iter().find(|b| b.vertex_iter().any(|x| Some(x.id()) == id)).cloned()
    };
    let start = if apex_start { None } else { ring_at(verts.first()) };
    let end = if apex_end { None } else { ring_at(verts.last()) };
    if (!apex_start && start.is_none()) || (!apex_end && end.is_none()) {
        return Err(fail("a turned run lost a ring"));
    }
    Ok(Built { faces, start, end })
}

fn corner_at(c: Vec2, z: f64) -> mt::Vertex {
    builder::vertex(p3(Vec3::new(c.x, c.y, z)))
}

/// The plain prism, by `tsweep` of its base ring.
fn prism(corners: &[Vec2], z0: f64, z1: f64, numbers: &[usize]) -> Result<Built> {
    let n = corners.len();
    if n < 3 || numbers.len() != n {
        return Err(fail("a prism needs its corners and a number per side"));
    }
    let verts: Vec<mt::Vertex> = corners.iter().map(|c| corner_at(*c, z0)).collect();
    let mut edges = Vec::with_capacity(n);
    for i in 0..n {
        let (Some(a), Some(b)) = (verts.get(i), verts.get((i + 1) % n)) else { continue };
        edges.push(builder::line(a, b));
    }
    let by_edge: HashMap<mt::EdgeID, usize> = edges.iter().map(|e| e.id()).zip(numbers.iter().copied()).collect();
    let base: mt::Wire = edges.into();
    let shell: mt::Shell = builder::tsweep(&base, v3(Vec3::Z * (z1 - z0)));
    let mut faces = Vec::with_capacity(n);
    for f in shell.face_iter() {
        let number = f.absolute_boundaries().iter().flatten().find_map(|e| by_edge.get(&e.id()).copied());
        let number = number.ok_or_else(|| fail("a prism side lost its segment"))?;
        faces.push((f.clone(), record::profile(&[number])));
    }
    let boundaries = shell.extract_boundaries();
    let z_of = |b: &mt::Wire| b.vertex_iter().next().map_or(f64::NAN, |v| v.point().z);
    let ring = |z: f64| boundaries.iter().find(|b| (z_of(b) - z).abs() < 1e-9).cloned();
    let (start, end) = (ring(z0), ring(z1));
    if start.is_none() || end.is_none() {
        return Err(fail("a prism lost a ring"));
    }
    Ok(Built { faces, start, end })
}

/// One cone end of a coned prism, after the ring on its end plane is known.
struct ConeEnd {
    /// The circle on the end plane.
    circle: mt::Wire,
    /// The hyperbolas on the flats: edge `i` runs from corner `i` to corner `i + 1`.
    hyperbolas: Vec<mt::Edge>,
    /// The corners on the flats' edges.
    corners: Vec<mt::Vertex>,
    /// The cone's sectors.
    faces: Vec<(mt::Face, FaceProv)>,
}

/// The cone cutting the end at height `z`; `inward` is the unit step along Z into the prism.
fn cone_end(corners: &[Vec2], z: f64, inward: f64, cone: &AxialCone) -> Result<ConeEnd> {
    let n = corners.len();
    let rc = corners.first().map_or(0.0, |c| c.len());
    let height = |rho: f64| z + inward * (rho - cone.radius) * cone.slope;
    let z_corner = height(rc);
    let verts: Vec<mt::Vertex> = corners.iter().map(|c| corner_at(*c, z_corner)).collect();
    // The circle's points lie on the same meridians as the corners, so a ruling joins each
    // pair and every sector lies in one sheet of the cone's parameters.
    let circle_verts: Vec<mt::Vertex> = corners
        .iter()
        .map(|c| {
            let u = *c * (cone.radius / rc);
            corner_at(u, z)
        })
        .collect();
    let mut arcs = Vec::with_capacity(n);
    let mut rulings = Vec::with_capacity(n);
    let mut hyperbolas = Vec::with_capacity(n);
    for i in 0..n {
        let j = (i + 1) % n;
        let (Some(ca), Some(cb), Some(va), Some(vb), Some(ta), Some(tb)) =
            (corners.get(i), corners.get(j), verts.get(i), verts.get(j), circle_verts.get(i), circle_verts.get(j))
        else {
            return Err(fail("a cone end's corners"));
        };
        let mid = (*ca + *cb) * 0.5;
        let apothem = mid.len();
        if cone.radius >= apothem - 1e-9 {
            return Err(fail("the cone's circle reaches past the flats"));
        }
        let dir = mid * (1.0 / apothem);
        let on_circle = dir * cone.radius;
        arcs.push(builder::circle_arc(ta, tb, p3(Vec3::new(on_circle.x, on_circle.y, z))));
        rulings.push(builder::line(ta, va));
        // The flat meets the cone in a hyperbola, one exact rational quadratic: the tangents at
        // its ends meet on the flat's middle; the weight puts the curve through its vertex there.
        let half = ca.dist(*cb) * 0.5;
        let z_mid = height(apothem);
        let z_apex = z_corner - inward * cone.slope * half * half / rc;
        let w = (z_mid - z_corner) / (z_apex - z_mid);
        if !(w.is_finite() && w > 0.0) {
            return Err(fail("the cone does not cut the flat"));
        }
        let (a, b) = (va.point(), vb.point());
        let ctrl = vec![
            mt::Vector4::new(a.x, a.y, a.z, 1.0),
            mt::Vector4::new(mid.x * w, mid.y * w, z_apex * w, w),
            mt::Vector4::new(b.x, b.y, b.z, 1.0),
        ];
        let curve = mt::NurbsCurve::new(mt::BSplineCurve::new(mt::KnotVec::bezier_knot(2), ctrl));
        hyperbolas.push(mt::Edge::new(va, vb, mt::Curve::NurbsCurve(curve)));
    }
    // The cone as a surface of revolution of its generator (same as a revolved line), running
    // past the corners a little so no boundary lies on the parameter's end.
    let axis = mt::Vector3::new(0.0, 0.0, 1.0);
    let far = rc * 1.01;
    let generator = mt::Curve::Line(mt::Line(
        mt::Point3::new(cone.radius * 0.99, 0.0, height(cone.radius * 0.99)),
        mt::Point3::new(far, 0.0, height(far)),
    ));
    let surface = mt::Surface::RevolutedCurve(mt::Processor::new(mt::RevolutedCurve::by_revolution(generator, mt::Point3::new(0.0, 0.0, 0.0), axis)));
    let mut faces = Vec::with_capacity(n);
    for i in 0..n {
        let j = (i + 1) % n;
        let (Some(rule_a), Some(hyper), Some(rule_b), Some(arc)) = (rulings.get(i), hyperbolas.get(i), rulings.get(j), arcs.get(i)) else {
            return Err(fail("a cone sector's edges"));
        };
        let wire: mt::Wire = vec![rule_a.clone(), hyper.clone(), rule_b.inverse(), arc.inverse()].into();
        let face = mt::Face::try_new(vec![wire], surface.clone()).map_err(|e| fail(&format!("a cone sector: {e}")))?;
        faces.push((face, record::profile(&[cone.number])));
    }
    let circle: mt::Wire = arcs.into();
    Ok(ConeEnd { circle, hyperbolas, corners: verts, faces })
}

/// A prism with a cone at one end or both, face by face.
fn coned_prism(corners: &[Vec2], z0: f64, z1: f64, numbers: &[usize], at0: Option<&AxialCone>, at1: Option<&AxialCone>) -> Result<Built> {
    let n = corners.len();
    if n < 3 || numbers.len() != n {
        return Err(fail("a prism needs its corners and a number per side"));
    }
    let toward1 = (z1 - z0).signum();
    let end0 = at0.map(|c| cone_end(corners, z0, toward1, c)).transpose()?;
    let end1 = at1.map(|c| cone_end(corners, z1, -toward1, c)).transpose()?;
    // A plain end: its corners and the straight edges between them.
    let plain = |z: f64| -> (Vec<mt::Vertex>, Vec<mt::Edge>) {
        let verts: Vec<mt::Vertex> = corners.iter().map(|c| corner_at(*c, z)).collect();
        let edges = (0..n).filter_map(|i| Some(builder::line(verts.get(i)?, verts.get((i + 1) % n)?))).collect();
        (verts, edges)
    };
    let (v0, e0) = match &end0 {
        Some(e) => (e.corners.clone(), e.hyperbolas.clone()),
        None => plain(z0),
    };
    let (v1, e1) = match &end1 {
        Some(e) => (e.corners.clone(), e.hyperbolas.clone()),
        None => plain(z1),
    };
    let risers: Vec<mt::Edge> = (0..n).filter_map(|i| Some(builder::line(v0.get(i)?, v1.get(i)?))).collect();
    let mut faces = Vec::new();
    for i in 0..n {
        let j = (i + 1) % n;
        let (Some(a), Some(up_j), Some(b), Some(up_i), Some(&number)) = (e0.get(i), risers.get(j), e1.get(i), risers.get(i), numbers.get(i)) else {
            return Err(fail("a flat's edges"));
        };
        let wire: mt::Wire = vec![a.clone(), up_j.clone(), b.inverse(), up_i.inverse()].into();
        faces.push((planar(&[wire])?, record::profile(&[number])));
    }
    let ring_of = |edges: &[mt::Edge]| -> mt::Wire { edges.to_vec().into() };
    let (start, end) = (
        end0.as_ref().map_or_else(|| ring_of(&e0), |e| e.circle.clone()),
        end1.as_ref().map_or_else(|| ring_of(&e1), |e| e.circle.clone()),
    );
    for e in [end0, end1].into_iter().flatten() {
        faces.extend(e.faces);
    }
    Ok(Built { faces, start: Some(start), end: Some(end) })
}

fn planar(wires: &[mt::Wire]) -> Result<mt::Face> {
    builder::try_attach_plane(wires).map_err(|e| fail(&format!("a planar face: {e}")))
}

/// A ring's mean radius about Z and its signed area about +Z, from each edge's ends and middle.
fn ring_measure(w: &mt::Wire) -> (f64, f64) {
    let mut pts: Vec<Vec3> = Vec::new();
    for e in w.edge_iter() {
        let c = e.oriented_curve();
        let (t0, t1) = c.range_tuple();
        pts.push(from_p3(c.subs(t0)));
        pts.push(from_p3(c.subs((t0 + t1) * 0.5)));
    }
    let n = pts.len().max(1) as f64;
    let radius = pts.iter().map(|p| (p.x * p.x + p.y * p.y).sqrt()).sum::<f64>() / n;
    let mut area = 0.0;
    for (k, p) in pts.iter().enumerate() {
        if let Some(q) = pts.get((k + 1) % pts.len()) {
            area += p.x * q.y - q.x * p.y;
        }
    }
    (radius, area * 0.5)
}

/// A ring's height.
fn ring_height(w: &mt::Wire) -> f64 {
    w.vertex_iter().next().map_or(f64::NAN, |v| v.point().z)
}

/// A planar face: its outer ring, the inner ring about the axis if any, and holes.
fn with_holes(outer: &mt::Wire, inner: Option<&mt::Wire>, holes: &[mt::Wire]) -> Result<mt::Face> {
    let turned = |w: &mt::Wire, positive: bool| {
        if (ring_measure(w).1 > 0.0) == positive { w.clone() } else { w.inverse() }
    };
    let mut wires = vec![turned(outer, true)];
    wires.extend(inner.map(|w| turned(w, false)));
    wires.extend(holes.iter().map(|w| turned(w, false)));
    planar(&wires)
}

/// A hole's wall: two half cylinders, with its rings at its lower and upper height.
fn drill(d: &AxialDrill) -> Result<(Vec<(mt::Face, FaceProv)>, mt::Wire, mt::Wire)> {
    if d.radius <= 0.0 || (d.z1 - d.z0).abs() < 1e-9 {
        return Err(fail("a hole needs a radius and a length"));
    }
    let (low, high) = (d.z0.min(d.z1), d.z0.max(d.z1));
    let on = |dx: f64, dy: f64| Vec3::new(d.center.x + dx * d.radius, d.center.y + dy * d.radius, low);
    let (a, b) = (builder::vertex(p3(on(1.0, 0.0))), builder::vertex(p3(on(-1.0, 0.0))));
    let base: mt::Wire = vec![builder::circle_arc(&a, &b, p3(on(0.0, 1.0))), builder::circle_arc(&b, &a, p3(on(0.0, -1.0)))].into();
    let shell: mt::Shell = builder::tsweep(&base, v3(Vec3::Z * (high - low)));
    let walls = shell.face_iter().map(|f| (f.clone(), record::profile(&[d.number]))).collect();
    let boundaries = shell.extract_boundaries();
    let ring = |z: f64| boundaries.iter().find(|w| (ring_height(w) - z).abs() < 1e-9).cloned();
    match (ring(low), ring(high)) {
        (Some(l), Some(h)) => Ok((walls, l, h)),
        _ => Err(fail("a hole lost a ring")),
    }
}

/// The faces turned so that every edge is run once each way (truck's sweeps and planes come in
/// either sense).
fn oriented(mut faces: Vec<(mt::Face, FaceProv)>) -> Result<Vec<(mt::Face, FaceProv)>> {
    let mut by_edge: HashMap<mt::EdgeID, Vec<(usize, bool)>> = HashMap::new();
    for (k, (f, _)) in faces.iter().enumerate() {
        for e in f.boundaries().iter().flatten() {
            by_edge.entry(e.id()).or_default().push((k, e.orientation()));
        }
    }
    // Whether each face must turn, spreading from the first over shared edges.
    let mut turn: Vec<Option<bool>> = vec![None; faces.len()];
    let mut todo = vec![0usize];
    if let Some(t) = turn.first_mut() {
        *t = Some(false);
    }
    while let Some(k) = todo.pop() {
        let Some((f, _)) = faces.get(k) else { continue };
        let here = turn.get(k).copied().flatten().unwrap_or(false);
        for e in f.boundaries().iter().flatten() {
            for &(other, sense) in by_edge.get(&e.id()).into_iter().flatten() {
                if other == k {
                    continue;
                }
                let need = here ^ (sense == e.orientation());
                match turn.get(other).copied().flatten() {
                    None => {
                        if let Some(t) = turn.get_mut(other) {
                            *t = Some(need);
                        }
                        todo.push(other);
                    }
                    Some(t) if t != need => return Err(fail("the faces cannot be oriented alike")),
                    Some(_) => {}
                }
            }
        }
    }
    for (k, (f, _)) in faces.iter_mut().enumerate() {
        match turn.get(k).copied().flatten() {
            Some(true) => {
                f.invert();
            }
            Some(false) => {}
            None => return Err(fail("the faces are not all connected")),
        }
    }
    Ok(faces)
}

/// The profile numbers of a tee's faces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TeeNumbers {
    /// The run's outside.
    pub run: usize,
    /// The branch's outside.
    pub branch: usize,
    /// The run's bore.
    pub run_bore: usize,
    /// The branch's bore.
    pub branch_bore: usize,
    /// The run's end on the XY plane.
    pub low_end: usize,
    /// The run's other end.
    pub high_end: usize,
    /// The branch's end.
    pub branch_end: usize,
}

/// One cylinder of a tee (the outside or the bore), its faces and its end rings.
struct TeeShell {
    faces: Vec<(mt::Face, FaceProv)>,
    low: mt::Wire,
    high: mt::Wire,
    side: mt::Wire,
}

/// A cylinder as a surface of revolution of a line parallel to its axis, the parameter's seam on
/// the side away from the face: `at` a point of the line, a little longer than the faces.
fn tee_surface(at: Vec3, axis: Vec3, centre: Vec3, reach: f64) -> mt::Surface {
    // Run against the axis, its normal points away from the axis: the walls below are drawn
    // counter-clockwise seen from outside.
    let generator = mt::Curve::Line(mt::Line(p3(at + axis * reach), p3(at - axis * reach)));
    mt::Surface::RevolutedCurve(mt::Processor::new(mt::RevolutedCurve::by_revolution(generator, p3(centre), v3(axis))))
}

/// A quarter of a tee's ellipse, one exact rational quadratic.
fn tee_quarter(a: &mt::Vertex, corner: Vec3, b: &mt::Vertex) -> mt::Edge {
    let (p, q) = (a.point(), b.point());
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let ctrl = vec![mt::Vector4::new(p.x, p.y, p.z, 1.0), mt::Vector4::new(corner.x * w, corner.y * w, corner.z * w, w), mt::Vector4::new(q.x, q.y, q.z, 1.0)];
    mt::Edge::new(a, b, mt::Curve::NurbsCurve(mt::NurbsCurve::new(mt::BSplineCurve::new(mt::KnotVec::bezier_knot(2), ctrl))))
}

fn tee_shell(r: f64, c: f64, run: usize, branch: usize) -> Result<TeeShell> {
    let v = |x: f64, y: f64, z: f64| builder::vertex(p3(Vec3::new(x, y, z)));
    let top = 2.0 * c;
    let (bn, bp) = (v(0.0, -r, 0.0), v(0.0, r, 0.0));
    let (tn, tp) = (v(0.0, -r, top), v(0.0, r, top));
    let (mn, mp) = (v(0.0, -r, c), v(0.0, r, c));
    let (el, eu) = (v(r, 0.0, c - r), v(r, 0.0, c + r));
    let (xn, xp) = (v(c, -r, c), v(c, r, c));
    let arc = |a: &mt::Vertex, b: &mt::Vertex, through: Vec3| builder::circle_arc(a, b, p3(through));
    // The run's end circles, a back half (x < 0) and a front half.
    let low_back = arc(&bp, &bn, Vec3::new(-r, 0.0, 0.0));
    let low_front = arc(&bn, &bp, Vec3::new(r, 0.0, 0.0));
    let high_back = arc(&tp, &tn, Vec3::new(-r, 0.0, top));
    let high_front = arc(&tn, &tp, Vec3::new(r, 0.0, top));
    // The run's sides, broken where the ellipses end.
    let (n_low, n_high) = (builder::line(&bn, &mn), builder::line(&mn, &tn));
    let (p_low, p_high) = (builder::line(&bp, &mp), builder::line(&mp, &tp));
    // The two halves of the ellipses in the planes z - c = -x and z - c = x.
    let lower_a = tee_quarter(&mn, Vec3::new(r, -r, c - r), &el);
    let lower_b = tee_quarter(&el, Vec3::new(r, r, c - r), &mp);
    let upper_a = tee_quarter(&mn, Vec3::new(r, -r, c + r), &eu);
    let upper_b = tee_quarter(&eu, Vec3::new(r, r, c + r), &mp);
    // The branch's sides and its end circle.
    let (n_side, p_side) = (builder::line(&mn, &xn), builder::line(&mp, &xp));
    let side_up = arc(&xn, &xp, Vec3::new(c, 0.0, c + r));
    let side_down = arc(&xp, &xn, Vec3::new(c, 0.0, c - r));
    let (z, x) = (Vec3::Z, Vec3::X);
    let run_centre = Vec3::new(0.0, 0.0, c);
    let branch_centre = Vec3::new(c / 2.0, 0.0, c);
    let reach = c * 1.2;
    let face = |wire: Vec<mt::Edge>, surface: mt::Surface, number: usize| -> Result<(mt::Face, FaceProv)> {
        let wire: mt::Wire = wire.into();
        let f = mt::Face::try_new(vec![wire], surface).map_err(|e| fail(&format!("a tee's face: {e}")))?;
        Ok((f, record::profile(&[number])))
    };
    let faces = vec![
        // Behind the branch: the parameter's seam in front of it.
        face(
            vec![low_back.clone(), n_low.clone(), n_high.clone(), high_back.inverse(), p_high.inverse(), p_low.inverse()],
            tee_surface(Vec3::new(r, 0.0, c), z, run_centre, reach),
            run,
        )?,
        // Below and above the branch: the seam behind.
        face(
            vec![low_front.clone(), p_low.clone(), lower_b.inverse(), lower_a.inverse(), n_low.inverse()],
            tee_surface(Vec3::new(-r, 0.0, c), z, run_centre, reach),
            run,
        )?,
        face(
            vec![upper_a.clone(), upper_b.clone(), p_high.clone(), high_front.inverse(), n_high.inverse()],
            tee_surface(Vec3::new(-r, 0.0, c), z, run_centre, reach),
            run,
        )?,
        // The branch above and below its middle: the seam on the other side.
        face(
            vec![n_side.clone(), side_up.clone(), p_side.inverse(), upper_b.inverse(), upper_a.inverse()],
            tee_surface(Vec3::new(c / 2.0, 0.0, c - r), x, branch_centre, reach),
            branch,
        )?,
        face(
            vec![lower_a.clone(), lower_b.clone(), p_side.clone(), side_down.clone(), n_side.inverse()],
            tee_surface(Vec3::new(c / 2.0, 0.0, c + r), x, branch_centre, reach),
            branch,
        )?,
    ];
    Ok(TeeShell {
        faces,
        low: vec![low_back, low_front].into(),
        high: vec![high_front, high_back].into(),
        side: vec![side_up, side_down].into(),
    })
}

/// A planar ring between two wires, the signs by the given plane's two coordinates.
fn tee_ring(outer: &mt::Wire, inner: &mt::Wire) -> Result<mt::Face> {
    planar(&[outer.clone(), inner.inverse()])
}

/// A straight tee of pipe, built face by face: the run along +Z from the XY plane with its middle at
/// height `c`, the branch from there along +X, each end `c` from the middle. Equal cylinders meet in
/// two exact half ellipses touching at their ends, the case the general Boolean is slowest at (25 s).
pub fn pipe_tee(radius: f64, bore: f64, c: f64, numbers: TeeNumbers) -> Result<Body> {
    if !(bore > 0.0 && bore < radius && radius < c) {
        return Err(fail("a tee needs a bore inside its radius and ends beyond it"));
    }
    guard("tee", || {
        let outer = tee_shell(radius, c, numbers.run, numbers.branch)?;
        let inner = tee_shell(bore, c, numbers.run_bore, numbers.branch_bore)?;
        let mut faces = outer.faces;
        faces.extend(inner.faces);
        faces.push((tee_ring(&outer.low, &inner.low)?, record::profile(&[numbers.low_end])));
        faces.push((tee_ring(&outer.high, &inner.high)?, record::profile(&[numbers.high_end])));
        faces.push((tee_ring(&outer.side, &inner.side)?, record::profile(&[numbers.branch_end])));
        let (faces, history): (Vec<mt::Face>, Vec<FaceProv>) = oriented(faces)?.into_iter().unzip();
        let shell: mt::Shell = faces.into();
        let solid = Solid::try_new(vec![shell]).map_err(|e| fail(&format!("the shell does not close: {e}")))?;
        let body = match bottom_faces_out(&solid) {
            Some(out) => Body::new_placed(solid, !out, None, false)?,
            None => Body::new(solid)?,
        };
        record::attach(body, &[], Some(history), Default::default())
    })
}
