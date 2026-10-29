//! Cross-part references (spec 3.10 §3.3, T3): geometry of another component
//! of an assembly that a part was built against while edited in place — a
//! sketch on another part's face, for now.
//!
//! A reference keeps its source (the assembly, the occurrence paths, the
//! face's persistent name) **and a copy of the geometry in this part's
//! coordinates**. The part regenerates from the copy only (rule 8: nothing
//! is looked up while it builds, the part opened alone keeps its last
//! state); the assembly writes new copies on Update
//! (`assembly::context::update`).

use serde::{Deserialize, Serialize};

use crate::document::DocumentId;
use crate::kernel::{Plane, Tolerance};

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
    /// The face's persistent name in that component's part.
    pub face: String,
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
}

impl ContextGeom {
    /// The plane, for a sketch placed on it.
    pub fn plane(&self) -> &Plane {
        match self {
            ContextGeom::Plane { plane } => plane,
        }
    }

    /// Whether `other` is the same geometry (Update writes only changes).
    pub fn same_as(&self, other: &ContextGeom) -> bool {
        let tol = Tolerance::default();
        let (a, b) = (self.plane(), other.plane());
        tol.points_coincide(&a.origin, &b.origin)
            && (a.normal.into_inner() - b.normal.into_inner()).norm() < tol.angular
            && (a.x_axis.into_inner() - b.x_axis.into_inner()).norm() < tol.angular
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
        self.work.context.insert(id.clone(), c.geom.plane().clone());
        self.context.push(c);
        id
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
