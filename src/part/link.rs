//! **Project Geometry** of sketch geometry and curved model edges
//! (`docs/spec/1.8-project-geometry.md`).
//!
//! A [`GeometryLink`] ties geometry of one sketch (the *target*) to its
//! source: a point, line, circle, arc or spline of another sketch of the
//! same part (by sketch id + point/entity id, never by index), or a curved
//! model edge (by [`EdgeRef`]: the names of its two faces and a hint; a
//! circle or arc stays one, any other curve becomes an interpolation spline
//! through [`CURVE_POINTS`] of its points). The
//! target geometry is fixed (points fixed, circle radius held) and is moved
//! to the orthogonal projection of the source onto the target plane at
//! every regeneration ([`apply`]), so user geometry built on it follows.
//!
//! Rules:
//! - a sketch projects only from sketches *before* it in the history
//!   ([`order_key`]); otherwise the link is sick;
//! - circles and arcs project only onto a parallel plane (else they would be
//!   ellipses, not supported yet);
//! - a missing source leaves the geometry frozen and the sketch sick
//!   (`PartSketch::health`), never silently changed.
//!
//! Model points (`PartSketch::projections`) and straight model edges
//! (`PartSketch::reference_lines`) keep their older representation.

use serde::{Deserialize, Serialize};

use super::{EdgeRef, PartSketch};
use crate::kernel::{BRep, Curve, Plane, Point3, Real, Tolerance, UnitVector3, Vector3};
use crate::sketch::{Entity, EntityId, Point2, PointId, Sketch, SplineFit, Style, Vector2};

/// What of a source sketch is projected.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum SketchGeo {
    /// A sketch point (a centre point, a line end, a spline point…).
    Point {
        /// The point in the source sketch.
        point: PointId,
    },
    /// A curve: line, circle, arc or spline.
    Curve {
        /// The curve in the source sketch.
        entity: EntityId,
    },
}

/// Where a projection comes from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum LinkSource {
    /// Geometry of another sketch of this part.
    Sketch {
        /// `id` of the source sketch.
        sketch: String,
        /// The projected point or curve.
        geo: SketchGeo,
    },
    /// A circular model edge (circle or arc).
    Edge {
        /// The edge.
        edge: EdgeRef,
    },
    /// A work feature (format revision 16): a work point → a point, a work
    /// axis → a line along it, a work plane → its line of intersection with
    /// the sketch plane.
    Work {
        /// The work feature id.
        id: String,
    },
}

impl LinkSource {
    /// The source sketch id, if it is sketch geometry.
    pub fn sketch(&self) -> Option<&str> {
        match self {
            LinkSource::Sketch { sketch, .. } => Some(sketch),
            LinkSource::Edge { .. } | LinkSource::Work { .. } => None,
        }
    }
}

/// One projection into a sketch (Properties → References row).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GeometryLink {
    /// The source.
    pub source: LinkSource,
    /// The target sketch points, in the source's point order: a point
    /// `[p]`; a line `[start, end]`; a circle `[center]`; an arc
    /// `[center, start, end]` (of the source); a spline its distinct points
    /// in order.
    pub points: Vec<PointId>,
    /// The target curve (`None` for a point).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve: Option<EntityId>,
    /// Problem at the last update (source missing…). Runtime only.
    #[serde(skip)]
    pub status: Option<String>,
}

/// A source resolved in model coordinates.
#[derive(Clone, Debug, PartialEq)]
pub enum Shape3 {
    /// A point.
    Point(Point3),
    /// A segment.
    Line(Point3, Point3),
    /// A full circle (counter-clockwise about `normal`).
    Circle {
        /// Centre.
        center: Point3,
        /// Axis.
        normal: UnitVector3,
        /// Radius.
        radius: Real,
    },
    /// An arc counter-clockwise about `normal` from `start` to `end`.
    Arc {
        /// Centre.
        center: Point3,
        /// Axis.
        normal: UnitVector3,
        /// First end.
        start: Point3,
        /// Second end.
        end: Point3,
    },
    /// A spline: its distinct points, the point list as indices into them,
    /// and the fit data of an interpolation spline with the source plane's
    /// axes (its handles are 2D vectors in that plane).
    Spline {
        /// Distinct points in order of first use.
        points: Vec<Point3>,
        /// The point list (a closed spline repeats its first point).
        order: Vec<usize>,
        /// Interpolation fit data and the source frame `(x, y)`.
        fit: Option<(SplineFit, Vector3, Vector3)>,
    },
}

impl Shape3 {
    /// Its points in link order.
    fn points(&self) -> Vec<Point3> {
        match self {
            Shape3::Point(p) => vec![*p],
            Shape3::Line(a, b) => vec![*a, *b],
            Shape3::Circle { center, .. } => vec![*center],
            Shape3::Arc {
                center, start, end, ..
            } => vec![*center, *start, *end],
            Shape3::Spline { points, .. } => points.clone(),
        }
    }

    /// The kind as the References list names it.
    pub fn kind(&self) -> &'static str {
        match self {
            Shape3::Point(_) => "Point",
            Shape3::Line(..) => "Line",
            Shape3::Circle { .. } => "Circle",
            Shape3::Arc { .. } => "Arc",
            Shape3::Spline { .. } => "Spline",
        }
    }
}

/// History order of sketch `i`: sketches with a smaller key come first
/// (they are resolved earlier in [`super::PartDocument::regenerate`]).
pub fn order_key(sketches: &[PartSketch], i: usize) -> (usize, usize) {
    (
        sketches
            .get(i)
            .and_then(|s| s.after_features)
            .unwrap_or(usize::MAX),
        i,
    )
}

/// The geometry `geo` of `sketch` in model coordinates.
pub fn sketch_shape(sketch: &Sketch, geo: SketchGeo) -> Result<Shape3, String> {
    let world = |p: PointId| {
        sketch
            .point(p)
            .map(|q| sketch.to_world(&q.position))
            .ok_or_else(|| "the projected geometry no longer exists in its sketch".to_owned())
    };
    let normal = sketch.plane.normal;
    match geo {
        SketchGeo::Point { point } => Ok(Shape3::Point(world(point)?)),
        SketchGeo::Curve { entity } => match sketch.entity(entity) {
            None => Err("the projected curve no longer exists in its sketch".into()),
            Some(Entity::Line { start, end }) => Ok(Shape3::Line(world(*start)?, world(*end)?)),
            Some(Entity::Circle { center, radius }) => Ok(Shape3::Circle {
                center: world(*center)?,
                normal,
                radius: *radius,
            }),
            Some(Entity::Arc { center, start, end }) => Ok(Shape3::Arc {
                center: world(*center)?,
                normal,
                start: world(*start)?,
                end: world(*end)?,
            }),
            Some(Entity::Spline { points: list }) => {
                let mut ids: Vec<PointId> = Vec::new();
                let mut order = Vec::with_capacity(list.len());
                for &p in list {
                    let k = match ids.iter().position(|&q| q == p) {
                        Some(k) => k,
                        None => {
                            ids.push(p);
                            ids.len() - 1
                        }
                    };
                    order.push(k);
                }
                let points = ids.iter().map(|&p| world(p)).collect::<Result<_, _>>()?;
                let fit = sketch.spline_fit(entity).cloned().map(|f| {
                    let x = sketch.plane.x_axis.into_inner();
                    (f, x, sketch.plane.normal.cross(&x))
                });
                Ok(Shape3::Spline { points, order, fit })
            }
        },
    }
}

/// How many points an open curved edge that is no circle projects through
/// (a closed one: twice as many). Fixed, so the target keeps its points
/// when the edge changes shape.
pub const CURVE_POINTS: usize = 9;

/// A model edge that is neither straight nor circular, as an interpolation
/// spline through points evenly spaced along it (natural ends).
fn curve_shape(body: &BRep, e: crate::kernel::EdgeId, closed: bool) -> Result<Shape3, String> {
    let pts = body
        .sample_edge(e, &crate::kernel::TessellationOptions::default())
        .map_err(|e| e.to_string())?;
    if pts.len() < 2 {
        return Err("the edge has no length".into());
    }
    // Arc length at each sample.
    let mut at = vec![0.0];
    for w in pts.windows(2) {
        at.push(at[at.len() - 1] + (w[1] - w[0]).norm());
    }
    let total = at[at.len() - 1];
    if total <= 1e-9 {
        return Err("the edge has no length".into());
    }
    let k = if closed {
        2 * CURVE_POINTS
    } else {
        CURVE_POINTS
    };
    // Distinct points: open curves include both ends; closed ones stop
    // before the end (it is the start again).
    let steps = if closed { k } else { k - 1 };
    let mut out = Vec::with_capacity(k);
    let mut j = 0;
    for i in 0..k {
        let s = total * i as Real / steps as Real;
        while j + 1 < at.len() - 1 && at[j + 1] < s {
            j += 1;
        }
        let span = (at[j + 1] - at[j]).max(1e-300);
        let t = ((s - at[j]) / span).clamp(0.0, 1.0);
        out.push(pts[j] + (pts[j + 1] - pts[j]) * t);
    }
    if !closed {
        out[0] = pts[0];
        out[k - 1] = pts[pts.len() - 1];
    }
    let mut order: Vec<usize> = (0..k).collect();
    if closed {
        order.push(0);
    }
    let fit = SplineFit {
        start: None,
        end: None,
        inner: Vec::new(),
        closed,
    };
    Ok(Shape3::Spline {
        points: out,
        order,
        fit: Some((fit, Vector3::x(), Vector3::y())),
    })
}

/// A curved model edge in model coordinates: a circle or arc, or any other
/// curve as an interpolation spline ([`curve_shape`]).
pub fn edge_shape(body: &BRep, edge: &EdgeRef) -> Result<Shape3, String> {
    let e = super::draft::edge_of(body, edge).ok_or_else(|| {
        format!(
            "the edge between {} and {} no longer exists",
            edge.faces[0], edge.faces[1]
        )
    })?;
    let x = body.edge(e).ok_or("the edge no longer exists")?;
    let Some(Curve::Circle(c)) = body.curve(x.curve) else {
        if body.edge_direction(e).is_some() {
            return Err("the edge is straight now; project it again".into());
        }
        return curve_shape(body, e, x.start == x.end);
    };
    let (a, b) = (
        body.vertex(x.start).map(|v| v.point),
        body.vertex(x.end).map(|v| v.point),
    );
    let full = x.start == x.end || (x.range.max - x.range.min) >= std::f64::consts::TAU - 1e-9;
    match (a, b) {
        (Some(start), Some(end)) if !full => Ok(Shape3::Arc {
            center: c.center,
            normal: c.normal,
            start,
            end,
        }),
        _ => Ok(Shape3::Circle {
            center: c.center,
            normal: c.normal,
            radius: c.radius,
        }),
    }
}

/// The source of `link` resolved for target sketch `i` of `sketches`.
pub fn resolve(
    source: &LinkSource,
    sketches: &[PartSketch],
    i: usize,
    body: &BRep,
    works: &super::placement::WorkGeom,
) -> Result<Shape3, String> {
    match source {
        LinkSource::Work { id } => {
            let plane = &sketches.get(i).ok_or("no such sketch")?.sketch.plane;
            work_shape(works, id, plane, body)
        }
        LinkSource::Sketch { sketch, geo } => {
            let k = sketches
                .iter()
                .position(|s| &s.id == sketch)
                .ok_or_else(|| format!("sketch {sketch} no longer exists"))?;
            if k == i {
                return Err("a sketch cannot project its own geometry".into());
            }
            if order_key(sketches, k) >= order_key(sketches, i) {
                return Err(format!(
                    "{} comes after this sketch in the history",
                    sketches[k].name
                ));
            }
            sketch_shape(&sketches[k].sketch, *geo)
        }
        LinkSource::Edge { edge } => edge_shape(body, edge),
    }
}

/// A work feature as a shape for sketch `plane`: a point; an axis as a
/// segment along it (half a model diagonal each way, 50 mm at least); a
/// plane as the segment of its intersection line with the sketch plane.
pub fn work_shape(
    works: &super::placement::WorkGeom,
    id: &str,
    plane: &Plane,
    body: &BRep,
) -> Result<Shape3, String> {
    let half = body
        .bounding_box()
        .map_or(50.0, |(a, b)| ((b - a).norm() * 0.5).max(50.0));
    if let Some(p) = works.points.get(id) {
        return Ok(Shape3::Point(*p));
    }
    if let Some((p, d)) = works.axes.get(id) {
        let d = d
            .try_normalize(1e-12)
            .ok_or("the work axis has no direction")?;
        // Centred on the sketch: the point of the axis nearest its origin.
        let c = p + d * (plane.origin - p).dot(&d);
        return Ok(Shape3::Line(c - d * half, c + d * half));
    }
    if let Some(w) = works.planes.get(id) {
        let (n1, n2) = (plane.normal.into_inner(), w.normal.into_inner());
        let dir = n1.cross(&n2);
        let s = dir.norm();
        if s < 1e-9 {
            return Err("the work plane is parallel to the sketch: they do not cross".into());
        }
        let dir = dir / s;
        // A point on both planes: solve in the span of the two normals.
        let (h1, h2) = (n1.dot(&plane.origin.coords), n2.dot(&w.origin.coords));
        let k = n1.dot(&n2);
        let det = 1.0 - k * k;
        let (a, b) = ((h1 - k * h2) / det, (h2 - k * h1) / det);
        let q = Point3::from(n1 * a + n2 * b);
        let c = q + dir * (plane.origin - q).dot(&dir);
        return Ok(Shape3::Line(c - dir * half, c + dir * half));
    }
    Err(format!("work feature {id} no longer exists"))
}

/// `shape` on `plane` (sketch coordinates); circles and arcs need a
/// parallel plane.
#[derive(Clone, Debug, PartialEq)]
pub struct Projected {
    /// The points in link order.
    pub points: Vec<Point2>,
    /// A circle's radius.
    pub radius: Option<Real>,
    /// An arc that runs clockwise on the target plane (the source faces
    /// the other way): its target arc goes from the source's end to start.
    pub flipped: bool,
    /// A spline's point list (indices into `points`) and fit data mapped
    /// into the target plane.
    pub spline: Option<(Vec<usize>, Option<SplineFit>)>,
}

/// Projects `shape` orthogonally onto `plane`.
pub fn project(shape: &Shape3, plane: &Plane) -> Result<Projected, String> {
    let tol = Tolerance::default();
    let points: Vec<Point2> = shape.points().iter().map(|p| plane.project(p)).collect();
    let mut out = Projected {
        points,
        radius: None,
        flipped: false,
        spline: None,
    };
    match shape {
        Shape3::Point(_) => {}
        Shape3::Line(..) => {
            if (out.points[1] - out.points[0]).norm() <= tol.linear {
                return Err(
                    "the line is square to the sketch plane (it projects to a point)".into(),
                );
            }
        }
        Shape3::Circle { normal, radius, .. } => {
            if !tol.are_parallel(normal, &plane.normal) {
                return Err(
                    "the circle is not parallel to the sketch plane (an ellipse is not supported yet; project its centre)"
                        .into(),
                );
            }
            out.radius = Some(*radius);
        }
        Shape3::Arc { normal, .. } => {
            if !tol.are_parallel(normal, &plane.normal) {
                return Err(
                    "the arc is not parallel to the sketch plane (an elliptical arc is not supported yet; project its centre)"
                        .into(),
                );
            }
            out.flipped = normal.dot(&plane.normal) < 0.0;
            // The centre moved onto the bisector of the two ends: a model
            // edge's vertices sit up to the kernel's tolerance off its circle
            // (1.75e-6 mm at a shelled lid's rounded corner), and an arc
            // whose ends are not equally far from its centre leaves a gap
            // the exact kernel refuses ("profile loop is not closed").
            let (c, s, e) = (out.points[0], out.points[1], out.points[2]);
            let chord = e - s;
            if chord.norm() > tol.linear {
                let m = nalgebra::center(&s, &e);
                let n = Vector2::new(-chord.y, chord.x).normalize();
                out.points[0] = m + n * (c - m).dot(&n);
            }
        }
        Shape3::Spline { order, fit, .. } => {
            let y = plane.normal.cross(&plane.x_axis);
            let fit = fit.as_ref().map(|(f, sx, sy)| {
                let map = |h: Option<[Real; 2]>| {
                    h.map(|h| {
                        let v = sx * h[0] + sy * h[1];
                        [v.dot(&plane.x_axis), v.dot(&y)]
                    })
                };
                SplineFit {
                    start: map(f.start),
                    end: map(f.end),
                    inner: f.inner.iter().map(|&h| map(h)).collect(),
                    closed: f.closed,
                }
            });
            out.spline = Some((order.clone(), fit));
        }
    }
    Ok(out)
}

/// Creates the fixed target geometry of `proj` in `sketch`; returns
/// `(points, curve)` for a new [`GeometryLink`].
pub fn create(
    sketch: &mut Sketch,
    shape: &Shape3,
    proj: &Projected,
    construction: bool,
) -> Result<(Vec<PointId>, Option<EntityId>), String> {
    create_with(sketch, shape, proj, construction, &[])
}

/// [`create`], taking existing points for some of the link's points
/// (`reuse`: index in link order → point): the face outline projected when
/// a sketch is made shares its corners with the straight edges, so the
/// loop is closed. Points made here are removed again on failure; reused
/// ones are left alone.
pub fn create_with(
    sketch: &mut Sketch,
    shape: &Shape3,
    proj: &Projected,
    construction: bool,
    reuse: &[(usize, PointId)],
) -> Result<(Vec<PointId>, Option<EntityId>), String> {
    let mut made_here = Vec::new();
    let pts: Vec<PointId> = proj
        .points
        .iter()
        .enumerate()
        .map(|(i, &p)| match reuse.iter().find(|(k, _)| *k == i) {
            Some(&(_, q)) => q,
            None => {
                let q = sketch.add_fixed_point(p);
                made_here.push(q);
                q
            }
        })
        .collect();
    let made = (|| -> Result<Option<EntityId>, crate::sketch::SketchError> {
        Ok(match shape {
            Shape3::Point(_) => None,
            Shape3::Line(..) => Some(sketch.add_line_between(pts[0], pts[1])?),
            Shape3::Circle { .. } => {
                let c = sketch.add_circle_at(pts[0], proj.radius.unwrap_or(1.0))?;
                sketch.set_radius_fixed(c, true)?;
                Some(c)
            }
            Shape3::Arc { .. } => Some(if proj.flipped {
                sketch.add_arc(pts[0], pts[2], pts[1])?
            } else {
                sketch.add_arc(pts[0], pts[1], pts[2])?
            }),
            Shape3::Spline { .. } => {
                let Some((order, fit)) = &proj.spline else {
                    return Err(crate::sketch::SketchError::InvalidValue("spline data"));
                };
                let list: Vec<PointId> =
                    order.iter().filter_map(|&k| pts.get(k).copied()).collect();
                Some(match fit {
                    Some(f) => {
                        let s = sketch.add_interp_spline(list)?;
                        sketch.set_spline_fit(s, f.clone())?;
                        s
                    }
                    None => sketch.add_spline(list)?,
                })
            }
        })
    })();
    match made {
        Ok(curve) => {
            if let Some(c) = curve
                && construction
            {
                let _ = sketch.set_style(c, Style::Construction);
            }
            Ok((pts, curve))
        }
        Err(e) => {
            let _ = pts;
            for p in made_here {
                let _ = sketch.remove_point(p);
            }
            Err(e.to_string())
        }
    }
}

/// Moves the target geometry of `link` to `proj` (an update). Errors when
/// the source changed shape in a way the target cannot follow.
pub fn place(sketch: &mut Sketch, link: &GeometryLink, proj: &Projected) -> Result<(), String> {
    if proj.points.len() != link.points.len() {
        return Err("the source has a different number of points now; project it again".into());
    }
    if let Some(Entity::Arc { start, .. }) = link.curve.and_then(|c| sketch.entity(c)) {
        let flipped_now = *start != link.points[1];
        if flipped_now != proj.flipped {
            return Err("the arc's source turned over; project it again".into());
        }
    }
    for (&p, &at) in link.points.iter().zip(&proj.points) {
        sketch.move_point(p, at).map_err(|e| e.to_string())?;
    }
    if let (Some(c), Some(r)) = (link.curve, proj.radius) {
        sketch.set_circle_radius(c, r).map_err(|e| e.to_string())?;
    }
    if let (Some(c), Some((_, Some(fit)))) = (link.curve, &proj.spline) {
        sketch
            .set_spline_fit(c, fit.clone())
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Updates every link of sketch `i` from its sources (regeneration step).
/// Links whose target geometry the user deleted are dropped; sick links
/// keep their geometry frozen and return their messages.
pub fn apply(
    sketches: &mut [PartSketch],
    i: usize,
    body: &BRep,
    works: &super::placement::WorkGeom,
) -> Vec<String> {
    let Some(ps) = sketches.get_mut(i) else {
        return Vec::new();
    };
    if ps.links.is_empty() {
        return Vec::new();
    }
    let sketch = &ps.sketch;
    ps.links.retain(|l| {
        l.points.iter().all(|&p| sketch.point(p).is_some())
            && l.curve.is_none_or(|c| sketch.entity(c).is_some())
    });
    let shapes: Vec<Result<Shape3, String>> = sketches[i]
        .links
        .iter()
        .map(|l| resolve(&l.source, sketches, i, body, works))
        .collect();
    let ps = &mut sketches[i];
    let plane = ps.sketch.plane.clone();
    let mut sick = Vec::new();
    for (l, shape) in ps.links.iter_mut().zip(shapes) {
        let r = shape
            .and_then(|s| project(&s, &plane))
            .and_then(|pr| place(&mut ps.sketch, l, &pr));
        l.status = r.err();
        if let Some(msg) = &l.status {
            sick.push(format!("{}: {msg}", describe_source(&l.source)));
        }
    }
    sick
}

/// Short text of a source, e.g. `Sketch1 · Circle` (kind filled in by
/// the caller when known).
pub fn describe_source(source: &LinkSource) -> String {
    match source {
        LinkSource::Sketch { sketch, .. } => sketch.clone(),
        LinkSource::Edge { edge } => format!("Edge {} / {}", edge.faces[0], edge.faces[1]),
        LinkSource::Work { id } => id.clone(),
    }
}
