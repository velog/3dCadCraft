//! Work features (`docs/spec/2.10-work-features.md`): work planes, axes and
//! points: helper geometry that is not part of the body. They are features
//! in the history, rebuilt in order at every regeneration from persistent
//! references into [`WorkGeom`]; sketches and other work features refer to
//! them ([`PlaneRef::Work`], [`LineRef::Work`], [`PointRef::Work`]).

use serde::{Deserialize, Serialize};

use super::params::FeatureParams;
use super::placement::{PlaneRef, PointRef, WorkGeom, plane_of, point_of};
use crate::kernel::{BRep, Curve, Plane, Point3, Real, Vector3};

/// What a work feature computation can look up.
pub struct Ctx<'a> {
    /// The body built so far.
    pub body: &'a BRep,
    /// The work features built so far.
    pub works: &'a WorkGeom,
    /// World axis of a Revolve feature by id (for "Through Revolved
    /// Feature").
    pub revolve_axis: &'a dyn Fn(&str) -> Option<(Point3, Vector3)>,
}

impl Ctx<'_> {
    fn plane(&self, r: &PlaneRef) -> Result<Plane, String> {
        plane_of(r, self.body, self.works).map_err(|e| e.to_string())
    }

    fn point(&self, r: &PointRef) -> Result<Point3, String> {
        point_of(r, self.body, self.works).map_err(|e| e.to_string())
    }

    fn line(&self, r: &LineRef) -> Result<(Point3, Vector3), String> {
        r.resolve(self.body, self.works)
    }
}

/// A straight line of the model: an origin axis, a straight edge or a work
/// axis.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LineRef {
    /// Origin axis: 0 = X, 1 = Y, 2 = Z.
    Origin(usize),
    /// A straight edge, by persistent name.
    Edge(String),
    /// A work axis, by the id of its feature.
    Work(String),
}

impl LineRef {
    /// A point on the line and its unit direction.
    pub fn resolve(&self, body: &BRep, works: &WorkGeom) -> Result<(Point3, Vector3), String> {
        match self {
            LineRef::Origin(k) => {
                let mut d = Vector3::zeros();
                d[(*k).min(2)] = 1.0;
                Ok((Point3::origin(), d))
            }
            LineRef::Edge(name) => {
                let e = body
                    .find_edge(name)
                    .ok_or_else(|| format!("edge '{name}' no longer exists"))?;
                let d = body
                    .edge_direction(e)
                    .ok_or_else(|| format!("edge '{name}' is not straight"))?;
                let start = body.edge(e).and_then(|x| body.vertex(x.start));
                let p = start.map(|v| v.point).ok_or("edge without vertices")?;
                Ok((p, d.normalize()))
            }
            LineRef::Work(id) => works
                .axes
                .get(id)
                .copied()
                .ok_or_else(|| format!("work feature '{id}' is not available")),
        }
    }

    /// Browser/Properties text.
    pub fn describe(&self) -> String {
        match self {
            LineRef::Origin(k) => format!("{} Axis", ["X", "Y", "Z"][(*k).min(2)]),
            LineRef::Edge(n) => format!("Edge {n}"),
            LineRef::Work(id) => id.replace("WorkAxis", "Work Axis"),
        }
    }
}

fn plane_make(o: Point3, n: Vector3, x: Vector3) -> Result<Plane, String> {
    Plane::new(o, n, x).map_err(|e| format!("degenerate plane: {e}"))
}

/// Any unit vector perpendicular to `n`.
fn perpendicular(n: &Vector3) -> Vector3 {
    let t = if n.x.abs() < 0.9 {
        Vector3::x()
    } else {
        Vector3::y()
    };
    (t - n * t.dot(n)).normalize()
}

// ---------------------------------------------------------------- planes ---

/// How a work plane is defined (Inventor's methods).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum WorkPlaneDef {
    /// **Offset from Plane**: parallel to `base` at `distance` along its
    /// normal (negative: the other side).
    Offset {
        /// The plane it is parallel to.
        base: PlaneRef,
        /// Signed distance (mm).
        distance: Real,
    },
    /// **Parallel to Plane Through Point**.
    ThroughPoint {
        /// The plane it is parallel to.
        base: PlaneRef,
        /// The point it passes through.
        point: PointRef,
    },
    /// **Midplane Between Two Planes** (parallel); normal of the first.
    Midplane {
        /// First plane (gives the normal).
        a: PlaneRef,
        /// Second plane.
        b: PlaneRef,
    },
    /// **Angle to Plane Around Edge**: through `axis` (parallel to
    /// `base`), turned by `angle` from `base`.
    Angle {
        /// The plane the angle is measured from.
        base: PlaneRef,
        /// The hinge line, parallel to `base`.
        axis: LineRef,
        /// Angle (radians).
        angle: Real,
    },
    /// **Three Points**: X from the first to the second, Y towards the third.
    ThreePoints {
        /// The three points, in pick order.
        points: [PointRef; 3],
    },
    /// **Two Coplanar Edges**: X along the first.
    TwoLines {
        /// First line (gives X).
        a: LineRef,
        /// Second line, in the same plane.
        b: LineRef,
    },
    /// **Normal to Axis Through Point**: X from the axis towards the point.
    NormalToAxis {
        /// The axis the plane is perpendicular to.
        axis: LineRef,
        /// The point it passes through.
        point: PointRef,
    },
    /// **Fixed**: at a position and orientation, not associative.
    Fixed {
        /// Origin.
        origin: [Real; 3],
        /// Normal (unit).
        normal: [Real; 3],
        /// X axis (unit, ⟂ normal).
        x_axis: [Real; 3],
    },
}

impl WorkPlaneDef {
    /// Inventor's method name.
    pub fn method(&self) -> &'static str {
        match self {
            WorkPlaneDef::Offset { .. } => "Offset from Plane",
            WorkPlaneDef::ThroughPoint { .. } => "Parallel to Plane Through Point",
            WorkPlaneDef::Midplane { .. } => "Midplane Between Two Planes",
            WorkPlaneDef::Angle { .. } => "Angle to Plane Around Edge",
            WorkPlaneDef::ThreePoints { .. } => "Three Points",
            WorkPlaneDef::TwoLines { .. } => "Two Coplanar Edges",
            WorkPlaneDef::NormalToAxis { .. } => "Normal to Axis Through Point",
            WorkPlaneDef::Fixed { .. } => "Fixed",
        }
    }

    /// The plane.
    pub fn compute(&self, c: &Ctx) -> Result<Plane, String> {
        match self {
            WorkPlaneDef::Offset { base, distance } => {
                let p = c.plane(base)?;
                let n = p.normal.into_inner();
                plane_make(p.origin + n * *distance, n, p.x_axis.into_inner())
            }
            WorkPlaneDef::ThroughPoint { base, point } => {
                let p = c.plane(base)?;
                let n = p.normal.into_inner();
                let q = c.point(point)?;
                plane_make(
                    p.origin + n * (q - p.origin).dot(&n),
                    n,
                    p.x_axis.into_inner(),
                )
            }
            WorkPlaneDef::Midplane { a, b } => {
                let (pa, pb) = (c.plane(a)?, c.plane(b)?);
                let (na, nb) = (pa.normal.into_inner(), pb.normal.into_inner());
                if na.cross(&nb).norm() > 1e-6 {
                    return Err("the planes are not parallel".into());
                }
                let gap = (pb.origin - pa.origin).dot(&na);
                plane_make(pa.origin + na * (gap / 2.0), na, pa.x_axis.into_inner())
            }
            WorkPlaneDef::Angle { base, axis, angle } => {
                let p = c.plane(base)?;
                let n = p.normal.into_inner();
                let (q, d) = c.line(axis)?;
                if d.dot(&n).abs() > 1e-6 {
                    return Err("the edge is not parallel to the plane".into());
                }
                // Turn the normal about the edge.
                let nn = n * angle.cos() + d.cross(&n) * angle.sin();
                plane_make(q, nn, d)
            }
            WorkPlaneDef::ThreePoints { points } => {
                let [a, b, d] = [
                    c.point(&points[0])?,
                    c.point(&points[1])?,
                    c.point(&points[2])?,
                ];
                let x = b - a;
                let n = x.cross(&(d - a));
                if x.norm() < 1e-9 || n.norm() < 1e-9 * x.norm().max(1.0) {
                    return Err("the points are collinear".into());
                }
                plane_make(a, n.normalize(), x.normalize())
            }
            WorkPlaneDef::TwoLines { a, b } => {
                let ((pa, da), (pb, db)) = (c.line(a)?, c.line(b)?);
                // The normal: across the two directions, or (parallel lines)
                // across the first and the gap between them.
                let mut n = da.cross(&db);
                if n.norm() < 1e-9 {
                    n = da.cross(&(pb - pa));
                }
                if n.norm() < 1e-9 {
                    return Err("the lines are the same line".into());
                }
                let n = n.normalize();
                if (pb - pa).dot(&n).abs() > 1e-6 * (pb - pa).norm().max(1.0) {
                    return Err("the edges are not coplanar".into());
                }
                plane_make(pa, n, da)
            }
            WorkPlaneDef::NormalToAxis { axis, point } => {
                let (p, d) = c.line(axis)?;
                let q = c.point(point)?;
                let foot = p + d * (q - p).dot(&d);
                let off = q - foot;
                let x = if off.norm() > 1e-9 {
                    off.normalize()
                } else {
                    perpendicular(&d)
                };
                plane_make(q, d, x)
            }
            WorkPlaneDef::Fixed {
                origin,
                normal,
                x_axis,
            } => plane_make(
                Point3::from(*origin),
                Vector3::from(*normal),
                Vector3::from(*x_axis),
            ),
        }
    }
}

// ------------------------------------------------------------------ axes ---

/// How a work axis is defined.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum WorkAxisDef {
    /// **On Line or Edge**.
    OnLine {
        /// The line.
        line: LineRef,
    },
    /// **Parallel to Line Through Point**.
    ParallelThroughPoint {
        /// The direction.
        line: LineRef,
        /// The point.
        point: PointRef,
    },
    /// **Through Two Points**: + from the first to the second.
    TwoPoints {
        /// Start.
        a: PointRef,
        /// End.
        b: PointRef,
    },
    /// **Intersection of Two Planes**.
    TwoPlanes {
        /// First plane.
        a: PlaneRef,
        /// Second plane.
        b: PlaneRef,
    },
    /// **Normal to Plane Through Point**.
    NormalToPlane {
        /// The plane.
        plane: PlaneRef,
        /// The point.
        point: PointRef,
    },
    /// **Through Center of Circular Edge**.
    CircularEdge {
        /// A circular edge, by persistent name.
        edge: String,
    },
    /// **Through Revolved Feature**: its axis of revolution.
    Revolved {
        /// The Revolve feature id.
        feature: String,
    },
    /// A line projected onto a plane along the plane's normal.
    LineOnPlane {
        /// The line.
        line: LineRef,
        /// The plane.
        plane: PlaneRef,
    },
    /// **Fixed**: not associative.
    Fixed {
        /// A point on it.
        origin: [Real; 3],
        /// Direction (unit).
        dir: [Real; 3],
    },
}

impl WorkAxisDef {
    /// Inventor's method name.
    pub fn method(&self) -> &'static str {
        match self {
            WorkAxisDef::OnLine { .. } => "On Line or Edge",
            WorkAxisDef::ParallelThroughPoint { .. } => "Parallel to Line Through Point",
            WorkAxisDef::TwoPoints { .. } => "Through Two Points",
            WorkAxisDef::TwoPlanes { .. } => "Intersection of Two Planes",
            WorkAxisDef::NormalToPlane { .. } => "Normal to Plane Through Point",
            WorkAxisDef::CircularEdge { .. } => "Through Center of Circular Edge",
            WorkAxisDef::Revolved { .. } => "Through Revolved Feature",
            WorkAxisDef::LineOnPlane { .. } => "Line Projected onto Plane",
            WorkAxisDef::Fixed { .. } => "Fixed",
        }
    }

    /// The axis: a point on it and its unit direction.
    pub fn compute(&self, c: &Ctx) -> Result<(Point3, Vector3), String> {
        match self {
            WorkAxisDef::OnLine { line } => c.line(line),
            WorkAxisDef::ParallelThroughPoint { line, point } => {
                let (_, d) = c.line(line)?;
                Ok((c.point(point)?, d))
            }
            WorkAxisDef::TwoPoints { a, b } => {
                let (a, b) = (c.point(a)?, c.point(b)?);
                let d = b - a;
                if d.norm() < 1e-9 {
                    return Err("the points coincide".into());
                }
                Ok((a, d.normalize()))
            }
            WorkAxisDef::TwoPlanes { a, b } => {
                let (pa, pb) = (c.plane(a)?, c.plane(b)?);
                let (na, nb) = (pa.normal.into_inner(), pb.normal.into_inner());
                let d = na.cross(&nb);
                if d.norm() < 1e-9 {
                    return Err("the planes are parallel".into());
                }
                // The point of both planes nearest to the origin of the first.
                let (ha, hb) = (na.dot(&pa.origin.coords), nb.dot(&pb.origin.coords));
                let dd = d.norm_squared();
                let p = (nb.cross(&d) * ha + d.cross(&na) * hb) / dd;
                Ok((Point3::from(p), d.normalize()))
            }
            WorkAxisDef::NormalToPlane { plane, point } => {
                let p = c.plane(plane)?;
                Ok((c.point(point)?, p.normal.into_inner()))
            }
            WorkAxisDef::CircularEdge { edge } => {
                let e = c
                    .body
                    .find_edge(edge)
                    .and_then(|e| c.body.edge(e))
                    .ok_or_else(|| format!("edge '{edge}' no longer exists"))?;
                match c.body.curve(e.curve) {
                    Some(Curve::Circle(k)) => Ok((k.center, k.normal.into_inner())),
                    _ => Err(format!("edge '{edge}' is not circular")),
                }
            }
            WorkAxisDef::Revolved { feature } => (c.revolve_axis)(feature)
                .ok_or_else(|| format!("revolve '{feature}' is not available")),
            WorkAxisDef::LineOnPlane { line, plane } => {
                let (p, d) = c.line(line)?;
                let pl = c.plane(plane)?;
                let n = pl.normal.into_inner();
                let dp = d - n * d.dot(&n);
                if dp.norm() < 1e-9 {
                    return Err("the line is perpendicular to the plane".into());
                }
                Ok((p - n * (p - pl.origin).dot(&n), dp.normalize()))
            }
            WorkAxisDef::Fixed { origin, dir } => {
                let d = Vector3::from(*dir);
                if d.norm() < 1e-12 {
                    return Err("zero direction".into());
                }
                Ok((Point3::from(*origin), d.normalize()))
            }
        }
    }
}

// ---------------------------------------------------------------- points ---

/// How a work point is defined.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum WorkPointDef {
    /// **On Vertex, Sketch Point, or Midpoint**.
    OnPoint {
        /// The point.
        point: PointRef,
    },
    /// **Intersection of Three Planes**.
    ThreePlanes {
        /// The planes.
        planes: [PlaneRef; 3],
    },
    /// **Intersection of Two Lines**.
    TwoLines {
        /// First line.
        a: LineRef,
        /// Second line.
        b: LineRef,
    },
    /// **Intersection of Plane and Line**.
    PlaneLine {
        /// The plane.
        plane: PlaneRef,
        /// The line.
        line: LineRef,
    },
    /// **Center Point of Loop of Edges**: the centre of a circular edge, or
    /// the area centroid of the face loop the edge belongs to.
    LoopCenter {
        /// An edge of the loop, by persistent name.
        edge: String,
        /// The face whose loop is meant (an edge bounds two faces); `None`:
        /// the planar face with the largest loop.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        face: Option<String>,
    },
    /// **Grounded Point**: fixed in space at (x, y, z).
    Fixed {
        /// X (mm).
        x: Real,
        /// Y (mm).
        y: Real,
        /// Z (mm).
        z: Real,
    },
}

impl WorkPointDef {
    /// Inventor's method name.
    pub fn method(&self) -> &'static str {
        match self {
            WorkPointDef::OnPoint { .. } => "On Vertex or Midpoint",
            WorkPointDef::ThreePlanes { .. } => "Intersection of Three Planes",
            WorkPointDef::TwoLines { .. } => "Intersection of Two Lines",
            WorkPointDef::PlaneLine { .. } => "Intersection of Plane and Line",
            WorkPointDef::LoopCenter { .. } => "Center Point of Loop of Edges",
            WorkPointDef::Fixed { .. } => "Grounded Point",
        }
    }

    /// The point.
    pub fn compute(&self, c: &Ctx) -> Result<Point3, String> {
        match self {
            WorkPointDef::OnPoint { point } => c.point(point),
            WorkPointDef::ThreePlanes { planes } => {
                let p: Vec<Plane> = planes
                    .iter()
                    .map(|r| c.plane(r))
                    .collect::<Result<_, _>>()?;
                let n: Vec<Vector3> = p.iter().map(|q| q.normal.into_inner()).collect();
                let h: Vec<Real> = p
                    .iter()
                    .zip(&n)
                    .map(|(q, n)| n.dot(&q.origin.coords))
                    .collect();
                let det = n[0].dot(&n[1].cross(&n[2]));
                if det.abs() < 1e-9 {
                    return Err("the planes do not meet in one point".into());
                }
                let v = (n[1].cross(&n[2]) * h[0]
                    + n[2].cross(&n[0]) * h[1]
                    + n[0].cross(&n[1]) * h[2])
                    / det;
                Ok(Point3::from(v))
            }
            WorkPointDef::TwoLines { a, b } => {
                let ((p, u), (q, v)) = (c.line(a)?, c.line(b)?);
                let w = p - q;
                let (uu, uv, vv) = (u.dot(&u), u.dot(&v), v.dot(&v));
                let den = uu * vv - uv * uv;
                if den.abs() < 1e-12 {
                    return Err("the lines are parallel".into());
                }
                let s = (uv * v.dot(&w) - vv * u.dot(&w)) / den;
                let t = (uu * v.dot(&w) - uv * u.dot(&w)) / den;
                let (x, y) = (p + u * s, q + v * t);
                if (x - y).norm() > 1e-6 * (1.0 + x.coords.norm()) {
                    return Err("the lines do not intersect".into());
                }
                Ok(nalgebra::center(&x, &y))
            }
            WorkPointDef::PlaneLine { plane, line } => {
                let pl = c.plane(plane)?;
                let (p, d) = c.line(line)?;
                let n = pl.normal.into_inner();
                let den = d.dot(&n);
                if den.abs() < 1e-12 {
                    return Err("the line is parallel to the plane".into());
                }
                Ok(p + d * ((pl.origin - p).dot(&n) / den))
            }
            WorkPointDef::LoopCenter { edge, face } => loop_center(c.body, edge, face.as_deref()),
            WorkPointDef::Fixed { x, y, z } => Ok(Point3::new(*x, *y, *z)),
        }
    }
}

/// Centre of a circular edge, else the area centroid of the loop that
/// contains the edge on face `face` (by name), or on the planar face where
/// that loop is largest (never by face order, which the kernel decides).
fn loop_center(body: &BRep, edge: &str, face: Option<&str>) -> Result<Point3, String> {
    let id = body
        .find_edge(edge)
        .ok_or_else(|| format!("edge '{edge}' no longer exists"))?;
    let e = body.edge(id).ok_or("edge missing")?;
    if let Some(Curve::Circle(k)) = body.curve(e.curve)
        && e.start == e.end
    {
        return Ok(k.center);
    }
    let opts = crate::kernel::TessellationOptions::default();
    let mut best: Option<(Real, Point3)> = None;
    let mut first_err = None;
    for (fid, f) in body.faces() {
        if face.is_some_and(|n| f.name.as_deref() != Some(n)) {
            continue;
        }
        for lp in std::iter::once(&f.outer).chain(&f.inner) {
            if !lp.coedges.iter().any(|ce| ce.edge == id) {
                continue;
            }
            match loop_centroid(body, fid, lp, &opts) {
                Ok((area, p)) => {
                    if best.is_none_or(|(a, _)| area > a) {
                        best = Some((area, p));
                    }
                }
                Err(e) => {
                    first_err.get_or_insert(e);
                }
            }
        }
    }
    if let Some((_, p)) = best {
        return Ok(p);
    }
    if let Some(e) = first_err {
        return Err(e);
    }
    Err(format!("edge '{edge}' bounds no face"))
}

/// Area and area centroid of loop `lp` of planar face `fid`.
fn loop_centroid(
    body: &BRep,
    fid: crate::kernel::FaceId,
    lp: &crate::kernel::Loop,
    opts: &crate::kernel::TessellationOptions,
) -> Result<(Real, Point3), String> {
    let plane = body.face_plane(fid).ok_or("the loop is not planar")?;
    let mut pts: Vec<nalgebra::Point2<Real>> = Vec::new();
    for ce in &lp.coedges {
        let mut s = body.sample_edge(ce.edge, opts).map_err(|e| e.to_string())?;
        if ce.sense == crate::kernel::Sense::Reversed {
            s.reverse();
        }
        s.pop();
        pts.extend(s.iter().map(|p| plane.project(p)));
    }
    let (mut a, mut cx, mut cy) = (0.0, 0.0, 0.0);
    for i in 0..pts.len() {
        let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
        let cr = p.x * q.y - q.x * p.y;
        a += cr;
        cx += (p.x + q.x) * cr;
        cy += (p.y + q.y) * cr;
    }
    if a.abs() < 1e-12 {
        return Err("the loop has no area".into());
    }
    Ok((
        a.abs() / 2.0,
        plane.point_at(cx / (3.0 * a), cy / (3.0 * a)),
    ))
}

// -------------------------------------------------------------- features ---

/// The kinds of work feature.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum WorkDef {
    /// A work plane.
    Plane(WorkPlaneDef),
    /// A work axis.
    Axis(WorkAxisDef),
    /// A work point.
    Point(WorkPointDef),
}

impl WorkDef {
    /// Inventor's method name.
    pub fn method(&self) -> &'static str {
        match self {
            WorkDef::Plane(d) => d.method(),
            WorkDef::Axis(d) => d.method(),
            WorkDef::Point(d) => d.method(),
        }
    }

    /// Id / name prefix: `WorkPlane` / `Work Plane` etc.
    pub fn prefix(&self) -> (&'static str, &'static str) {
        match self {
            WorkDef::Plane(_) => ("WorkPlane", "Work Plane"),
            WorkDef::Axis(_) => ("WorkAxis", "Work Axis"),
            WorkDef::Point(_) => ("WorkPoint", "Work Point"),
        }
    }

    /// Computes it into `works` under `id`.
    pub fn build(&self, id: &str, c: &Ctx) -> Result<WorkResult, String> {
        Ok(match self {
            WorkDef::Plane(d) => WorkResult::Plane(id.to_owned(), d.compute(c)?),
            WorkDef::Axis(d) => {
                let (p, v) = d.compute(c)?;
                WorkResult::Axis(id.to_owned(), p, v)
            }
            WorkDef::Point(d) => WorkResult::Point(id.to_owned(), d.compute(c)?),
        })
    }
}

/// A computed work feature, to be stored in [`WorkGeom`].
pub enum WorkResult {
    /// (id, plane).
    Plane(String, Plane),
    /// (id, point on it, unit direction).
    Axis(String, Point3, Vector3),
    /// (id, position).
    Point(String, Point3),
}

impl WorkResult {
    /// Stores it.
    pub fn store(self, works: &mut WorkGeom) {
        match self {
            WorkResult::Plane(id, p) => {
                works.planes.insert(id, p);
            }
            WorkResult::Axis(id, p, v) => {
                works.axes.insert(id, (p, v));
            }
            WorkResult::Point(id, p) => {
                works.points.insert(id, p);
            }
        }
    }
}

/// A work plane, axis or point in the feature history.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkFeature {
    /// Immutable identity (`WorkPlane1`, `WorkAxis1`, `WorkPoint1`).
    pub id: String,
    /// Display name ("Work Plane1").
    pub name: String,
    /// The kind, method and inputs.
    pub def: WorkDef,
    /// Shown in the graphics window.
    #[serde(default = "yes")]
    pub visible: bool,
    /// Equations of the numeric fields (`distance`, `angle`, `x`/`y`/`z`).
    #[serde(default, skip_serializing_if = "FeatureParams::is_empty")]
    pub params: FeatureParams,
}

fn yes() -> bool {
    true
}

impl WorkFeature {
    /// A work feature with `def` (id and name are set when it is added).
    pub fn new(def: WorkDef) -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            def,
            visible: true,
            params: FeatureParams::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::part::{
        ExtrudeFeature, OriginPlane, PartDocument, ProfileRef, SketchOptions, SketchPlacement,
    };
    use crate::sketch::Point2;
    use approx::assert_relative_eq;

    fn boxed() -> PartDocument {
        PartDocument::with_box("Box1", [80.0, 60.0, 20.0]).unwrap()
    }

    fn no_revolve(_: &str) -> Option<(Point3, Vector3)> {
        None
    }

    fn v(n: &str) -> PointRef {
        PointRef::Vertex(format!("Box1:V{n}"))
    }

    #[test]
    fn plane_methods() {
        let part = boxed();
        let works = WorkGeom::new();
        let c = Ctx {
            body: &part.body,
            works: &works,
            revolve_axis: &no_revolve,
        };
        let top = PlaneRef::Face("Box1:Top".into());
        let p = WorkPlaneDef::Offset {
            base: top.clone(),
            distance: 15.0,
        }
        .compute(&c)
        .unwrap();
        assert_relative_eq!(p.origin.z, 35.0, epsilon = 1e-9);
        let p = WorkPlaneDef::Midplane {
            a: top.clone(),
            b: PlaneRef::Face("Box1:Bottom".into()),
        }
        .compute(&c)
        .unwrap();
        assert_relative_eq!(p.origin.z, 10.0, epsilon = 1e-9);
        assert_relative_eq!(p.normal.z, 1.0, epsilon = 1e-9);
        assert!(
            WorkPlaneDef::Midplane {
                a: top.clone(),
                b: PlaneRef::Origin(OriginPlane::YZ),
            }
            .compute(&c)
            .is_err()
        );
        let p = WorkPlaneDef::ThroughPoint {
            base: PlaneRef::Origin(OriginPlane::XY),
            point: v("111"),
        }
        .compute(&c)
        .unwrap();
        assert_relative_eq!(p.origin.z, 20.0, epsilon = 1e-9);
        let p = WorkPlaneDef::Angle {
            base: PlaneRef::Origin(OriginPlane::XY),
            axis: LineRef::Origin(0),
            angle: std::f64::consts::FRAC_PI_4,
        }
        .compute(&c)
        .unwrap();
        assert_relative_eq!(p.normal.z, 0.5f64.sqrt(), epsilon = 1e-9);
        assert!(
            WorkPlaneDef::Angle {
                base: PlaneRef::Origin(OriginPlane::XY),
                axis: LineRef::Origin(2),
                angle: 1.0,
            }
            .compute(&c)
            .is_err()
        );
        let p = WorkPlaneDef::ThreePoints {
            points: [v("001"), v("101"), v("111")],
        }
        .compute(&c)
        .unwrap();
        assert_relative_eq!(p.origin.z, 20.0, epsilon = 1e-9);
        assert_relative_eq!(p.x_axis.x, 1.0, epsilon = 1e-9);
        assert!(
            WorkPlaneDef::ThreePoints {
                points: [v("000"), v("100"), v("000")],
            }
            .compute(&c)
            .is_err()
        );
        // Two coplanar edges: the X and Y axes give the XY plane, X along X.
        let p = WorkPlaneDef::TwoLines {
            a: LineRef::Origin(0),
            b: LineRef::Origin(1),
        }
        .compute(&c)
        .unwrap();
        assert_relative_eq!(p.normal.z.abs(), 1.0, epsilon = 1e-9);
        assert_relative_eq!(p.x_axis.x, 1.0, epsilon = 1e-9);
        // Normal to the Z axis through the top corner: z = 20, X towards it.
        let p = WorkPlaneDef::NormalToAxis {
            axis: LineRef::Origin(2),
            point: v("111"),
        }
        .compute(&c)
        .unwrap();
        assert_relative_eq!(p.origin.z, 20.0, epsilon = 1e-9);
        assert_relative_eq!(p.x_axis.x, 0.8, epsilon = 1e-9);
    }

    #[test]
    fn axis_and_point_methods() {
        let part = boxed();
        let works = WorkGeom::new();
        let rev = |id: &str| (id == "Rev").then(|| (Point3::new(1.0, 2.0, 0.0), Vector3::z()));
        let c = Ctx {
            body: &part.body,
            works: &works,
            revolve_axis: &rev,
        };
        let face = |n: &str| PlaneRef::Face(format!("Box1:{n}"));
        // Intersection of the top and front faces: the top front edge.
        let (p, d) = WorkAxisDef::TwoPlanes {
            a: face("Top"),
            b: face("Front"),
        }
        .compute(&c)
        .unwrap();
        assert_relative_eq!(d.x.abs(), 1.0, epsilon = 1e-9);
        assert_relative_eq!(p.z, 20.0, epsilon = 1e-9);
        assert_relative_eq!(p.y, 0.0, epsilon = 1e-9);
        assert!(
            WorkAxisDef::TwoPlanes {
                a: face("Top"),
                b: face("Bottom"),
            }
            .compute(&c)
            .is_err()
        );
        let (p, d) = WorkAxisDef::TwoPoints {
            a: v("000"),
            b: v("111"),
        }
        .compute(&c)
        .unwrap();
        assert_relative_eq!(p.x, 0.0, epsilon = 1e-9);
        assert!(d.x > 0.0 && d.z > 0.0);
        let (p, d) = WorkAxisDef::NormalToPlane {
            plane: face("Top"),
            point: v("111"),
        }
        .compute(&c)
        .unwrap();
        assert_relative_eq!(d.z, 1.0, epsilon = 1e-9);
        assert_relative_eq!(p.x, 80.0, epsilon = 1e-9);
        let (_, d) = WorkAxisDef::ParallelThroughPoint {
            line: LineRef::Origin(1),
            point: v("101"),
        }
        .compute(&c)
        .unwrap();
        assert_relative_eq!(d.y, 1.0, epsilon = 1e-9);
        let (p, _) = WorkAxisDef::Revolved {
            feature: "Rev".into(),
        }
        .compute(&c)
        .unwrap();
        assert_relative_eq!(p.y, 2.0, epsilon = 1e-9);
        // The Z axis projected onto XY is no line.
        assert!(
            WorkAxisDef::LineOnPlane {
                line: LineRef::Origin(2),
                plane: PlaneRef::Origin(OriginPlane::XY),
            }
            .compute(&c)
            .is_err()
        );

        // Points.
        let p = WorkPointDef::ThreePlanes {
            planes: [face("Top"), face("Right"), face("Back")],
        }
        .compute(&c)
        .unwrap();
        assert_relative_eq!(
            (p - Point3::new(80.0, 60.0, 20.0)).norm(),
            0.0,
            epsilon = 1e-9
        );
        let p = WorkPointDef::TwoLines {
            a: LineRef::Origin(0),
            b: LineRef::Origin(2),
        }
        .compute(&c)
        .unwrap();
        assert_relative_eq!(p.coords.norm(), 0.0, epsilon = 1e-9);
        assert!(
            WorkPointDef::TwoLines {
                a: LineRef::Origin(0),
                b: LineRef::Origin(0),
            }
            .compute(&c)
            .is_err()
        );
        let p = WorkPointDef::PlaneLine {
            plane: face("Top"),
            line: LineRef::Origin(2),
        }
        .compute(&c)
        .unwrap();
        assert_relative_eq!(p.z, 20.0, epsilon = 1e-9);
        // The top face loop's centre.
        let top_edge = part
            .body
            .edges()
            .find_map(|(e, x)| {
                let n = x.name.clone()?;
                let f = part.body.edge_faces(e);
                f.iter()
                    .any(|f| part.body.face(*f).and_then(|f| f.name.as_deref()) == Some("Box1:Top"))
                    .then_some(n)
            })
            .unwrap();
        let p = WorkPointDef::LoopCenter {
            edge: top_edge,
            face: None,
        }
        .compute(&c)
        .unwrap();
        assert_relative_eq!(
            (p - Point3::new(40.0, 30.0, 20.0)).norm(),
            0.0,
            epsilon = 1e-6
        );
        let p = WorkPointDef::OnPoint {
            point: PointRef::EdgeMid(part.body.edges().find_map(|(_, x)| x.name.clone()).unwrap()),
        }
        .compute(&c);
        assert!(p.is_ok());
    }

    /// A sketch on a work plane offset from the top face follows the face
    /// when the box changes and survives save/load; work features build on
    /// each other (axis from two planes, point on the axis and a plane);
    /// a missing input makes the feature sick.
    #[test]
    fn work_features_in_the_history() {
        let mut part = boxed();
        let wp = part
            .add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
                base: PlaneRef::Face("Box1:Top".into()),
                distance: 15.0,
            })))
            .unwrap();
        assert_eq!(wp, "WorkPlane1");
        assert_eq!(part.features.last().unwrap().name(), "Work Plane1");
        let wa = part
            .add_work(WorkFeature::new(WorkDef::Axis(WorkAxisDef::TwoPlanes {
                a: PlaneRef::Work(wp.clone()),
                b: PlaneRef::Origin(OriginPlane::XZ),
            })))
            .unwrap();
        assert_eq!(wa, "WorkAxis1");
        let wpt = part
            .add_work(WorkFeature::new(WorkDef::Point(WorkPointDef::PlaneLine {
                plane: PlaneRef::Origin(OriginPlane::YZ),
                line: LineRef::Work(wa.clone()),
            })))
            .unwrap();
        assert_eq!(wpt, "WorkPoint1");
        assert_relative_eq!(
            (part.work.points[&wpt] - Point3::new(0.0, 0.0, 35.0)).norm(),
            0.0,
            epsilon = 1e-9
        );
        let placement = SketchPlacement {
            plane: PlaneRef::Work(wp.clone()),
            ..Default::default()
        };
        let i = part
            .create_sketch(placement, &SketchOptions::default())
            .unwrap();
        let sk = &mut part.sketches[i].sketch;
        sk.add_circle(Point2::new(10.0, 10.0), 5.0).unwrap();
        let r = sk.regions();
        let pr = ProfileRef::of(&r[0], r[0].interior_point());
        part.add_extrude(i, ExtrudeFeature::new(vec![pr], 5.0))
            .unwrap();
        // The box grows: everything follows.
        if let crate::part::Feature::Box(b) = &mut part.features[0] {
            b.size[2] = 30.0;
        }
        part.regenerate().unwrap();
        assert_relative_eq!(part.sketches[i].sketch.plane.origin.z, 45.0, epsilon = 1e-9);
        assert_relative_eq!(part.work.points[&wpt].z, 45.0, epsilon = 1e-9);
        assert!(part.feature_health.is_empty(), "{:?}", part.feature_health);
        // Save / load.
        let json = serde_json::to_string(&part).unwrap();
        let mut back: PartDocument = serde_json::from_str(&json).unwrap();
        back.regenerate().unwrap();
        assert_relative_eq!(back.work.planes[&wp].origin.z, 45.0, epsilon = 1e-9);
        // A missing base face: the plane, axis and point are sick; the
        // sketch keeps its last plane.
        if let crate::part::Feature::Work(w) = &mut part.features[1] {
            w.def = WorkDef::Plane(WorkPlaneDef::Offset {
                base: PlaneRef::Face("Gone:Top".into()),
                distance: 15.0,
            });
        }
        part.regenerate().unwrap();
        for id in [&wp, &wa, &wpt] {
            assert!(part.feature_health.contains_key(id), "{id}");
        }
        assert!(part.sketches[i].health.is_some());
        assert_relative_eq!(part.sketches[i].sketch.plane.origin.z, 45.0, epsilon = 1e-9);
    }
}
