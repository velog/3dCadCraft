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
    if runs.is_empty() {
        return Err(KernelError::Invalid("a stepped body needs a run".into()));
    }
    guard("stepped body", || {
        let built = runs.iter().map(|r| build_run(&r.shape)).collect::<Result<Vec<_>>>()?;
        let mut faces: Vec<(mt::Face, FaceProv)> = Vec::new();
        for b in &built {
            faces.extend(b.faces.iter().cloned());
        }
        if let (Some(n), false) = (first_cap, closed) {
            let ring = built.first().and_then(|b| b.start.clone()).ok_or_else(|| fail("the first run has no ring to close"))?;
            faces.push((planar(&[ring])?, record::profile(&[n])));
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
                (Some(a), Some(b), Some(n)) => faces.push((step_face(&a, &b)?, record::profile(&[n]))),
                (Some(a), None, Some(n)) if k + 1 == runs.len() => faces.push((planar(&[a])?, record::profile(&[n]))),
                (None, None, None) if k + 1 == runs.len() => {}
                _ => return Err(fail("the runs do not join")),
            }
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

/// The planar face between two rings at one height: the larger one outside.
fn step_face(a: &mt::Wire, b: &mt::Wire) -> Result<mt::Face> {
    let (ma, mb) = (ring_measure(a), ring_measure(b));
    let ((outer, mo), (inner, mi)) = if ma.0 >= mb.0 { ((a, ma), (b, mb)) } else { ((b, mb), (a, ma)) };
    let outer = if mo.1 > 0.0 { outer.clone() } else { outer.inverse() };
    let inner = if mi.1 < 0.0 { inner.clone() } else { inner.inverse() };
    planar(&[outer, inner])
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
