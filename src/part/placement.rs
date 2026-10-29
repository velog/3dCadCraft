//! Where a sketch lies: its plane and its 2D coordinate system.
//!
//! Mirrors how Inventor defines a planar sketch (`PlanarSketch` in its API):
//!
//! | Inventor property      | Here                          | Meaning                                        |
//! |------------------------|-------------------------------|------------------------------------------------|
//! | `PlanarEntity`         | [`SketchPlacement::plane`]    | origin plane, planar face (or work plane, later) |
//! | `OriginPoint`          | [`SketchPlacement::origin`]   | projected part origin or a model vertex        |
//! | `AxisEntity`           | [`SketchPlacement::axis_edge`]| straight model edge giving the axis direction  |
//! | `AxisIsX`              | [`SketchPlacement::axis_is_x`]| the edge defines X (true) or Y (false)         |
//! | `NaturalAxisDirection` | [`SketchPlacement::natural_direction`] | false flips that axis                 |
//!
//! All model references are **persistent names** (see `kernel::query`), so
//! [`SketchPlacement::resolve`] finds the same face, vertex and edge after
//! the body is regenerated — the sketch follows its face.
//!
//! ## Default coordinate system (no axis edge chosen)
//!
//! - **Origin planes** (Z up): XY → x = +X, y = +Y (seen from +Z);
//!   XZ → x = +X, y = +Z (seen from −Y, the FRONT view);
//!   YZ → x = +Y, y = +Z (seen from +X, the RIGHT view).
//! - **Faces**: the normal points out of the solid (you look at the face
//!   from outside). On a wall-like face (|n·Z| < 0.9) y is "up" (+Z
//!   projected), so text and dimensions read naturally; on a floor/ceiling
//!   face x is +X projected.
//! - **Origin**: the part origin projected onto the plane.
//!
//! Inventor's world is Y-up by default and its exact defaults differ in
//! places; any choice can be changed with Edit Coordinate System.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::kernel::{BRep, Plane, Point3, Vector3};

/// The three origin planes of a part.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OriginPlane {
    /// Normal +Z.
    XY,
    /// Normal −Y (front).
    XZ,
    /// Normal +X (right).
    YZ,
}

impl OriginPlane {
    /// All origin planes, in browser order.
    pub const ALL: [OriginPlane; 3] = [OriginPlane::YZ, OriginPlane::XZ, OriginPlane::XY];

    /// Browser label.
    pub fn label(self) -> &'static str {
        match self {
            OriginPlane::XY => "XY Plane",
            OriginPlane::XZ => "XZ Plane",
            OriginPlane::YZ => "YZ Plane",
        }
    }

    /// (normal, x axis) of the plane's sketch frame.
    pub fn frame(self) -> (Vector3, Vector3) {
        match self {
            OriginPlane::XY => (Vector3::z(), Vector3::x()),
            OriginPlane::XZ => (-Vector3::y(), Vector3::x()),
            OriginPlane::YZ => (Vector3::x(), Vector3::y()),
        }
    }
}

/// What a sketch is placed on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlaneRef {
    /// One of the part's origin planes.
    Origin(OriginPlane),
    /// A planar face of the body, by persistent name.
    Face(String),
    /// A work plane, by the id of its feature.
    Work(String),
    /// A face of another component of an assembly, by the id of the part's
    /// cross-part reference (`PartDocument::context`, spec 3.10 §3.3;
    /// format revision 44).
    Context(String),
    /// No model reference: the sketch keeps the plane stored in it
    /// (files written before placements existed, or detached sketches).
    Fixed,
}

/// A point of the model used as sketch origin or projected into a sketch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PointRef {
    /// The part's origin (0, 0, 0) — Inventor's "Center Point".
    PartOrigin,
    /// A model vertex, by persistent name.
    Vertex(String),
    /// The midpoint of a straight edge, by persistent name.
    EdgeMid(String),
    /// A work point, by the id of its feature.
    Work(String),
}

/// Complete definition of a sketch's plane and coordinate system.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SketchPlacement {
    /// The plane the sketch lies on.
    pub plane: PlaneRef,
    /// Origin of the sketch coordinate system (projected onto the plane).
    pub origin: PointRef,
    /// Straight model edge defining an axis direction; `None` = default.
    pub axis_edge: Option<String>,
    /// The axis edge defines X (`true`) or Y (`false`).
    pub axis_is_x: bool,
    /// `false` flips the axis (X, or Y when `axis_is_x` is false).
    pub natural_direction: bool,
}

impl Default for SketchPlacement {
    fn default() -> Self {
        Self {
            plane: PlaneRef::Fixed,
            origin: PointRef::PartOrigin,
            axis_edge: None,
            axis_is_x: true,
            natural_direction: true,
        }
    }
}

/// Why a placement cannot be resolved against the current body.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum PlacementError {
    /// The face no longer exists (e.g. removed by an edit).
    #[error("face '{0}' no longer exists")]
    FaceMissing(String),
    /// The face is not planar.
    #[error("face '{0}' is not planar")]
    NotPlanar(String),
    /// The origin vertex no longer exists.
    #[error("vertex '{0}' no longer exists")]
    VertexMissing(String),
    /// The axis edge no longer exists.
    #[error("edge '{0}' no longer exists")]
    EdgeMissing(String),
    /// Only straight edges can define an axis.
    #[error("edge '{0}' is not straight")]
    CurvedAxis(String),
    /// The axis edge is perpendicular to the sketch plane.
    #[error("edge '{0}' is perpendicular to the sketch plane")]
    AxisAlongNormal(String),
    /// The work plane is missing (deleted, or sick).
    #[error("work feature '{0}' is not available")]
    WorkFeatureMissing(String),
    /// The cross-part reference is gone (spec 3.10 §3.3).
    #[error("the reference '{0}' to another component no longer exists")]
    ContextMissing(String),
    /// The plane's frame could not be built (zero or parallel axes).
    #[error("degenerate sketch frame: {0}")]
    DegenerateFrame(String),
}

/// A kernel frame error as a placement error.
fn frame_error(e: crate::kernel::KernelError) -> PlacementError {
    PlacementError::DegenerateFrame(e.to_string())
}

/// The work features computed so far in a regeneration, by feature id:
/// planes, axes (a point and a unit direction) and points.
#[derive(Clone, Debug, Default)]
pub struct WorkGeom {
    /// Work planes.
    pub planes: std::collections::HashMap<String, Plane>,
    /// Work axes: (point on it, unit direction).
    pub axes: std::collections::HashMap<String, (Point3, Vector3)>,
    /// Work points.
    pub points: std::collections::HashMap<String, Point3>,
    /// The planes of the cross-part references (their copies).
    pub context: std::collections::HashMap<String, Plane>,
}

impl WorkGeom {
    /// Nothing computed yet.
    pub fn new() -> Self {
        Self::default()
    }
}

/// The plane `r` stands for (origin planes through the part origin, faces
/// with their outward normal, work planes as computed).
pub fn plane_of(r: &PlaneRef, body: &BRep, works: &WorkGeom) -> Result<Plane, PlacementError> {
    match r {
        PlaneRef::Origin(p) => {
            let (n, x) = p.frame();
            Plane::new(Point3::origin(), n, x).map_err(frame_error)
        }
        PlaneRef::Face(name) => {
            let id = body
                .find_face(name)
                .ok_or_else(|| PlacementError::FaceMissing(name.clone()))?;
            let plane = body
                .face_plane(id)
                .ok_or_else(|| PlacementError::NotPlanar(name.clone()))?;
            let n = plane.normal.into_inner();
            Plane::new(plane.origin, n, default_face_x(&n)).map_err(frame_error)
        }
        PlaneRef::Work(id) => works
            .planes
            .get(id)
            .cloned()
            .ok_or_else(|| PlacementError::WorkFeatureMissing(id.clone())),
        PlaneRef::Context(id) => works
            .context
            .get(id)
            .cloned()
            .ok_or_else(|| PlacementError::ContextMissing(id.clone())),
        PlaneRef::Fixed => Ok(Plane::xy()),
    }
}

/// A model point by reference.
pub fn point_of(r: &PointRef, body: &BRep, works: &WorkGeom) -> Result<Point3, PlacementError> {
    match r {
        PointRef::PartOrigin => Ok(Point3::origin()),
        PointRef::Vertex(name) => {
            let v = body
                .find_vertex(name)
                .ok_or_else(|| PlacementError::VertexMissing(name.clone()))?;
            body.vertex(v)
                .map(|v| v.point)
                .ok_or_else(|| PlacementError::VertexMissing(name.clone()))
        }
        PointRef::EdgeMid(name) => {
            let e = body
                .find_edge(name)
                .and_then(|e| body.edge(e))
                .ok_or_else(|| PlacementError::EdgeMissing(name.clone()))?;
            let (a, b) = (body.vertex(e.start), body.vertex(e.end));
            match (a, b) {
                (Some(a), Some(b)) => Ok(nalgebra::center(&a.point, &b.point)),
                _ => Err(PlacementError::EdgeMissing(name.clone())),
            }
        }
        PointRef::Work(id) => works
            .points
            .get(id)
            .copied()
            .ok_or_else(|| PlacementError::WorkFeatureMissing(id.clone())),
    }
}

impl PointRef {
    /// Browser / Properties text.
    pub fn describe(&self) -> String {
        match self {
            PointRef::PartOrigin => "Center Point".to_owned(),
            PointRef::Vertex(v) => format!("Vertex {v}"),
            PointRef::EdgeMid(e) => format!("Midpoint of {e}"),
            PointRef::Work(id) => id.replace("WorkPoint", "Work Point"),
        }
    }
}

impl SketchPlacement {
    /// Placement on an origin plane with the default coordinate system.
    pub fn on_origin_plane(plane: OriginPlane) -> Self {
        Self {
            plane: PlaneRef::Origin(plane),
            ..Default::default()
        }
    }

    /// Placement on a named face with the default coordinate system.
    pub fn on_face(name: impl Into<String>) -> Self {
        Self {
            plane: PlaneRef::Face(name.into()),
            ..Default::default()
        }
    }

    /// Human-readable description of the plane, for the browser and
    /// properties.
    pub fn describe(&self) -> String {
        match &self.plane {
            PlaneRef::Origin(p) => p.label().to_owned(),
            PlaneRef::Face(f) => format!("Face {f}"),
            PlaneRef::Work(id) => format!("Work plane {id}"),
            PlaneRef::Context(id) => format!("Face of another component ({id})"),
            PlaneRef::Fixed => "Fixed plane".to_owned(),
        }
    }

    /// Computes the sketch plane (origin, normal, x axis) against `body`.
    /// `current` is returned unchanged for [`PlaneRef::Fixed`].
    pub fn resolve(&self, body: &BRep, current: &Plane) -> Result<Plane, PlacementError> {
        self.resolve_with(body, &WorkGeom::new(), current)
    }

    /// [`Self::resolve`] with the work planes computed so far.
    pub fn resolve_with(
        &self,
        body: &BRep,
        works: &WorkGeom,
        current: &Plane,
    ) -> Result<Plane, PlacementError> {
        let (base, normal, default_x) = match &self.plane {
            PlaneRef::Fixed => return Ok(current.clone()),
            PlaneRef::Work(_) | PlaneRef::Context(_) => {
                let p = plane_of(&self.plane, body, works)?;
                (p.origin, p.normal.into_inner(), p.x_axis.into_inner())
            }
            PlaneRef::Origin(p) => {
                let (n, x) = p.frame();
                (Point3::origin(), n, x)
            }
            PlaneRef::Face(name) => {
                let id = body
                    .find_face(name)
                    .ok_or_else(|| PlacementError::FaceMissing(name.clone()))?;
                let plane = body
                    .face_plane(id)
                    .ok_or_else(|| PlacementError::NotPlanar(name.clone()))?;
                let n = plane.normal.into_inner();
                (plane.origin, n, default_face_x(&n))
            }
        };

        let origin_world = point_of(&self.origin, body, works)?;
        let origin = origin_world - normal * (origin_world - base).dot(&normal);

        let (mut axis, axis_is_x) = match &self.axis_edge {
            None => (default_x, true),
            Some(name) => {
                let e = body
                    .find_edge(name)
                    .ok_or_else(|| PlacementError::EdgeMissing(name.clone()))?;
                let dir = body
                    .edge_direction(e)
                    .ok_or_else(|| PlacementError::CurvedAxis(name.clone()))?;
                let in_plane = dir - normal * dir.dot(&normal);
                if in_plane.norm() < 1e-9 * dir.norm().max(1.0) {
                    return Err(PlacementError::AxisAlongNormal(name.clone()));
                }
                (in_plane.normalize(), self.axis_is_x)
            }
        };
        if !self.natural_direction {
            axis = -axis;
        }
        // If the axis is Y, X completes the right-handed frame: x = y × n.
        let x = if axis_is_x { axis } else { axis.cross(&normal) };
        Plane::new(origin, normal, x).map_err(frame_error)
    }
}

/// Default x axis for a sketch on a face with outward normal `n`.
pub(crate) fn default_face_x(n: &Vector3) -> Vector3 {
    if n.z.abs() < 0.9 {
        // Wall: y = world Z projected into the plane, x = y × n.
        let up = Vector3::z() - n * n.z;
        up.normalize().cross(n)
    } else {
        // Floor/ceiling: x = world X projected.
        let x = Vector3::x() - n * n.x;
        x.normalize()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    fn body() -> BRep {
        let mut b = BRep::new();
        b.make_box_named(Point3::origin(), Point3::new(80.0, 60.0, 20.0), "Box1")
            .unwrap();
        b
    }

    fn frame(p: &SketchPlacement) -> (Point3, Vector3, Vector3, Vector3) {
        let pl = p.resolve(&body(), &Plane::xy()).unwrap();
        (
            pl.origin,
            pl.x_axis.into_inner(),
            pl.y_axis().into_inner(),
            pl.normal.into_inner(),
        )
    }

    #[test]
    fn origin_plane_frames() {
        let (o, x, y, n) = frame(&SketchPlacement::on_origin_plane(OriginPlane::XZ));
        assert_eq!(o, Point3::origin());
        assert_relative_eq!(x, Vector3::x(), epsilon = 1e-12);
        assert_relative_eq!(y, Vector3::z(), epsilon = 1e-12);
        assert_relative_eq!(n, -Vector3::y(), epsilon = 1e-12);
        let (_, x, y, _) = frame(&SketchPlacement::on_origin_plane(OriginPlane::YZ));
        assert_relative_eq!(x, Vector3::y(), epsilon = 1e-12);
        assert_relative_eq!(y, Vector3::z(), epsilon = 1e-12);
    }

    #[test]
    fn face_frames_look_from_outside_with_y_up_on_walls() {
        // Top: normal +Z, origin = part origin projected to z = 20.
        let (o, x, _, n) = frame(&SketchPlacement::on_face("Box1:Top"));
        assert_relative_eq!(o, Point3::new(0.0, 0.0, 20.0), epsilon = 1e-12);
        assert_relative_eq!(x, Vector3::x(), epsilon = 1e-12);
        assert_relative_eq!(n, Vector3::z(), epsilon = 1e-12);
        // Every wall: y is +Z, normal points outward.
        for (face, normal) in [
            ("Front", -Vector3::y()),
            ("Back", Vector3::y()),
            ("Left", -Vector3::x()),
            ("Right", Vector3::x()),
        ] {
            let (_, _, y, n) = frame(&SketchPlacement::on_face(format!("Box1:{face}")));
            assert_relative_eq!(y, Vector3::z(), epsilon = 1e-12);
            assert_relative_eq!(n, normal, epsilon = 1e-12);
        }
    }

    #[test]
    fn coordinate_system_from_vertex_and_edge() {
        let mut p = SketchPlacement::on_face("Box1:Top");
        p.origin = PointRef::Vertex("Box1:V111".into());
        p.axis_edge = Some("Box1:E011-111".into()); // along +X
        p.axis_is_x = false; // …but make it the Y axis
        p.natural_direction = false; // …pointing −X
        let (o, x, y, _) = frame(&p);
        assert_relative_eq!(o, Point3::new(80.0, 60.0, 20.0), epsilon = 1e-12);
        assert_relative_eq!(y, -Vector3::x(), epsilon = 1e-12);
        assert_relative_eq!(x, Vector3::y(), epsilon = 1e-12); // x × y = +Z
    }

    #[test]
    fn missing_references_are_reported() {
        let b = body();
        let err = |p: SketchPlacement| p.resolve(&b, &Plane::xy()).unwrap_err();
        assert!(matches!(
            err(SketchPlacement::on_face("Box1:Nope")),
            PlacementError::FaceMissing(_)
        ));
        let mut p = SketchPlacement::on_face("Box1:Top");
        p.axis_edge = Some("Box1:E000-001".into()); // vertical edge ⟂ top face
        assert!(matches!(err(p), PlacementError::AxisAlongNormal(_)));
    }
}
