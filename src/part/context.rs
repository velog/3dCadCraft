//! Cross-part references (spec 3.10 §3.3, T3): geometry of another component
//! of an assembly that a part was built against while edited in place — a
//! sketch on another part's face, and another part's edges projected into a
//! sketch (format revision 47).
//!
//! A reference keeps its source (the assembly, the occurrence paths, the
//! face's persistent name) **and a copy of the geometry in this part's
//! coordinates**. The part regenerates from the copy only (rule 8: nothing
//! is looked up while it builds, the part opened alone keeps its last
//! state); the assembly writes new copies on Update
//! (`assembly::context::update`).

use serde::{Deserialize, Serialize};

use crate::document::DocumentId;
use crate::kernel::{Plane, Point3, Tolerance, Transform3, UnitVector3, Vector3};

/// One cross-part reference of a part.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContextRef {
    /// Immutable id (`Context1`…), what `PlaneRef::Context` names.
    pub id: String,
    /// Where the geometry comes from.
    pub source: ContextSource,
    /// The last copy of the geometry, in this part's coordinates.
    pub geom: ContextGeom,
}

/// The source of a cross-part reference.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContextSource {
    /// The assembly it was made in (the part is "adaptive in" it).
    pub assembly: DocumentId,
    /// Occurrence path of this part there (the copy is in its
    /// coordinates): the top-level id, then the ids inside subassemblies.
    pub target: Vec<u32>,
    /// Occurrence path of the component the geometry lies on.
    pub path: Vec<u32>,
    /// The face's persistent name in that component's part (for an edge:
    /// its first face).
    pub face: String,
    /// The edge, when the reference is an edge projected into a sketch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge: Option<super::EdgeRef>,
    /// A point on the face (the part's coordinates of the source).
    pub hint: [f64; 3],
    /// Shown name, `Base:1 face`.
    pub label: String,
}

/// The copied geometry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum ContextGeom {
    /// A planar face (its outward normal and a stable x axis).
    Plane {
        /// The plane.
        plane: Plane,
    },
    /// An edge (format revision 47), projected into a sketch through
    /// `LinkSource::Context`.
    Curve {
        /// The curve.
        curve: ContextCurve,
    },
}

/// An edge of another component in this part's coordinates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "curve")]
pub enum ContextCurve {
    /// A straight edge from `a` to `b`.
    Line {
        /// First end.
        a: Point3,
        /// Second end.
        b: Point3,
    },
    /// A whole circle.
    Circle {
        /// Centre.
        center: Point3,
        /// Axis (unit).
        normal: Vector3,
        /// Radius.
        radius: f64,
    },
    /// An arc counter-clockwise about `normal` from `start` to `end`.
    Arc {
        /// Centre.
        center: Point3,
        /// Axis (unit).
        normal: Vector3,
        /// First end.
        start: Point3,
        /// Second end.
        end: Point3,
    },
}

impl ContextCurve {
    /// From a model edge's shape: lines, circles and arcs (other curves are
    /// refused for now).
    pub fn of(shape: &super::link::Shape3) -> Result<Self, String> {
        use super::link::Shape3;
        Ok(match shape {
            Shape3::Line(a, b) => ContextCurve::Line { a: *a, b: *b },
            Shape3::Circle {
                center,
                normal,
                radius,
            } => ContextCurve::Circle {
                center: *center,
                normal: normal.into_inner(),
                radius: *radius,
            },
            Shape3::Arc {
                center,
                normal,
                start,
                end,
            } => ContextCurve::Arc {
                center: *center,
                normal: normal.into_inner(),
                start: *start,
                end: *end,
            },
            Shape3::Point(_) | Shape3::Spline { .. } => {
                return Err(
                    "only straight and circular edges of other components can be projected for now"
                        .into(),
                );
            }
        })
    }

    /// The shape a sketch link projects.
    pub fn shape(&self) -> super::link::Shape3 {
        use super::link::Shape3;
        let unit = |n: &Vector3| UnitVector3::new_normalize(*n);
        match self {
            ContextCurve::Line { a, b } => Shape3::Line(*a, *b),
            ContextCurve::Circle {
                center,
                normal,
                radius,
            } => Shape3::Circle {
                center: *center,
                normal: unit(normal),
                radius: *radius,
            },
            ContextCurve::Arc {
                center,
                normal,
                start,
                end,
            } => Shape3::Arc {
                center: *center,
                normal: unit(normal),
                start: *start,
                end: *end,
            },
        }
    }

    /// The curve moved by `m` (rigid).
    pub fn moved(&self, m: &Transform3) -> Self {
        match self {
            ContextCurve::Line { a, b } => ContextCurve::Line { a: m * a, b: m * b },
            ContextCurve::Circle {
                center,
                normal,
                radius,
            } => ContextCurve::Circle {
                center: m * center,
                normal: m * normal,
                radius: *radius,
            },
            ContextCurve::Arc {
                center,
                normal,
                start,
                end,
            } => ContextCurve::Arc {
                center: m * center,
                normal: m * normal,
                start: m * start,
                end: m * end,
            },
        }
    }

    fn same_as(&self, other: &ContextCurve) -> bool {
        let tol = Tolerance::default();
        let p = |a: &Point3, b: &Point3| tol.points_coincide(a, b);
        let v = |a: &Vector3, b: &Vector3| (a - b).norm() < tol.angular;
        match (self, other) {
            (ContextCurve::Line { a, b }, ContextCurve::Line { a: c, b: d }) => p(a, c) && p(b, d),
            (
                ContextCurve::Circle {
                    center,
                    normal,
                    radius,
                },
                ContextCurve::Circle {
                    center: c,
                    normal: n,
                    radius: r,
                },
            ) => p(center, c) && v(normal, n) && (radius - r).abs() < tol.linear,
            (
                ContextCurve::Arc {
                    center,
                    normal,
                    start,
                    end,
                },
                ContextCurve::Arc {
                    center: c,
                    normal: n,
                    start: s,
                    end: e,
                },
            ) => p(center, c) && v(normal, n) && p(start, s) && p(end, e),
            _ => false,
        }
    }
}

impl ContextGeom {
    /// The plane, for a sketch placed on it.
    pub fn plane(&self) -> Option<&Plane> {
        match self {
            ContextGeom::Plane { plane } => Some(plane),
            ContextGeom::Curve { .. } => None,
        }
    }

    /// The curve, for a sketch projecting it.
    pub fn curve(&self) -> Option<&ContextCurve> {
        match self {
            ContextGeom::Curve { curve } => Some(curve),
            ContextGeom::Plane { .. } => None,
        }
    }

    /// Whether `other` is the same geometry (Update writes only changes).
    pub fn same_as(&self, other: &ContextGeom) -> bool {
        let tol = Tolerance::default();
        match (self, other) {
            (ContextGeom::Plane { plane: a }, ContextGeom::Plane { plane: b }) => {
                tol.points_coincide(&a.origin, &b.origin)
                    && (a.normal.into_inner() - b.normal.into_inner()).norm() < tol.angular
                    && (a.x_axis.into_inner() - b.x_axis.into_inner()).norm() < tol.angular
            }
            (ContextGeom::Curve { curve: a }, ContextGeom::Curve { curve: b }) => a.same_as(b),
            _ => false,
        }
    }
}

impl super::PartDocument {
    /// The cross-part reference `id`.
    pub fn context_ref(&self, id: &str) -> Option<&ContextRef> {
        self.context.iter().find(|c| c.id == id)
    }

    /// Whether the sketch or feature `id` is adaptive (spec 3.10 §3.4).
    pub fn is_adaptive(&self, id: &str) -> bool {
        self.adaptive.iter().any(|a| a == id)
    }

    /// Makes the sketch or feature `id` adaptive or not; `true` when that
    /// changed it.
    pub fn set_adaptive(&mut self, id: &str, on: bool) -> bool {
        if self.is_adaptive(id) == on {
            return false;
        }
        if on {
            self.adaptive.push(id.to_owned());
        } else {
            self.adaptive.retain(|a| a != id);
        }
        true
    }

    /// Index of the sketch with id `id`.
    pub fn sketch_index(&self, id: &str) -> Option<usize> {
        self.sketches.iter().position(|s| s.id == id)
    }

    /// A free id for a new reference (`Context1`, `Context2`…).
    pub fn next_context_id(&self) -> String {
        (1..)
            .map(|k| format!("Context{k}"))
            .find(|id| self.context_ref(id).is_none())
            .unwrap_or_else(|| "Context".into())
    }

    /// Adds `c` (or finds the reference with the same source) and returns
    /// its id; its geometry is usable at once (no regeneration needed).
    pub fn add_context(&mut self, mut c: ContextRef) -> String {
        if let Some(old) = self.context.iter().find(|x| x.source == c.source) {
            return old.id.clone();
        }
        if c.id.is_empty() || self.context_ref(&c.id).is_some() {
            c.id = self.next_context_id();
        }
        let id = c.id.clone();
        self.work.add_context(&id, &c.geom);
        self.context.push(c);
        id
    }

    /// Project Geometry of another component's edge into sketch `index`
    /// (spec 3.10 §3.3): the reference `c` (a `ContextGeom::Curve`) is
    /// added and a link to it made; returns the link's index. Nothing
    /// changes on failure.
    pub fn project_context_edge(
        &mut self,
        index: usize,
        c: ContextRef,
        construction: bool,
    ) -> Result<usize, super::PartError> {
        if c.geom.curve().is_none() {
            return Err(super::PartError::Feature(
                "Project Geometry: the reference is not an edge".into(),
            ));
        }
        let before = (self.context.clone(), self.work.context_curves.clone());
        let id = self.add_context(c);
        let r = self.add_link(index, super::link::LinkSource::Context { id }, construction);
        if r.is_err() {
            (self.context, self.work.context_curves) = before;
        }
        r
    }

    /// Writes new copies (`id`, geometry); `true` when one changed (the
    /// part then needs a regeneration).
    pub fn set_context(&mut self, copies: &[(String, ContextGeom)]) -> bool {
        let mut changed = false;
        for (id, g) in copies {
            if let Some(c) = self.context.iter_mut().find(|c| c.id == *id)
                && !c.geom.same_as(g)
            {
                c.geom = g.clone();
                changed = true;
            }
        }
        changed
    }

    /// The regeneration cache's seed: the copies (a moved source must
    /// rebuild what lies on it). 0 without references, as before them.
    pub(super) fn context_key(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        if self.context.is_empty() {
            return 0;
        }
        let mut h = std::collections::hash_map::DefaultHasher::new();
        serde_json::to_string(&self.context)
            .unwrap_or_default()
            .hash(&mut h);
        h.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::{Point3, Vector3};
    use crate::part::{PartDocument, PlaneRef, SketchPlacement};

    /// The assembly every test reference was made in.
    fn assembly() -> DocumentId {
        static ID: std::sync::OnceLock<DocumentId> = std::sync::OnceLock::new();
        *ID.get_or_init(DocumentId::new_random)
    }

    fn reference(z: f64) -> ContextRef {
        ContextRef {
            id: String::new(),
            source: ContextSource {
                assembly: assembly(),
                target: vec![2],
                path: vec![1],
                face: "Box1:Top".into(),
                edge: None,
                hint: [5.0, 5.0, 10.0],
                label: "Base:1 face".into(),
            },
            geom: ContextGeom::Plane {
                plane: Plane::new(Point3::new(0.0, 0.0, z), Vector3::z(), Vector3::x()).unwrap(),
            },
        }
    }

    #[test]
    fn a_sketch_on_a_context_face_follows_its_copy() {
        let mut p = PartDocument::default();
        let id = p.add_context(reference(10.0));
        assert_eq!(id, "Context1");
        let placement = SketchPlacement {
            plane: PlaneRef::Context(id.clone()),
            ..Default::default()
        };
        let i = p.create_sketch(placement, &Default::default()).unwrap();
        assert!((p.sketches[i].sketch.plane.origin.z - 10.0).abs() < 1e-12);
        // The same source again is the same reference.
        assert_eq!(p.add_context(reference(10.0)), id);
        let n = p.sketches[i].sketch.plane.normal.into_inner();
        assert!((n - Vector3::z()).norm() < 1e-12);
        // Update writes a moved copy; the sketch follows at regeneration.
        let moved = reference(25.0).geom;
        assert!(p.set_context(&[(id.clone(), moved.clone())]));
        assert!(!p.set_context(&[(id, moved)]));
        p.regenerate().unwrap();
        assert!((p.sketches[i].sketch.plane.origin.z - 25.0).abs() < 1e-12);
    }

    #[test]
    fn references_round_trip_and_a_missing_one_makes_the_sketch_sick() {
        let mut p = PartDocument::default();
        let id = p.add_context(reference(4.0));
        let placement = SketchPlacement {
            plane: PlaneRef::Context(id),
            ..Default::default()
        };
        p.create_sketch(placement, &Default::default()).unwrap();
        let text = serde_json::to_string(&p).unwrap();
        let mut q: PartDocument = serde_json::from_str(&text).unwrap();
        q.regenerate().unwrap();
        assert_eq!(q.context, p.context);
        assert!((q.sketches[0].sketch.plane.origin.z - 4.0).abs() < 1e-12);
        // Rule 8: a reference that is gone leaves the sketch sick, in place.
        q.context.clear();
        q.regenerate().unwrap();
        assert!(q.sketches[0].health.is_some());
        assert!((q.sketches[0].sketch.plane.origin.z - 4.0).abs() < 1e-12);
    }
}
