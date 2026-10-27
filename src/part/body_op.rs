//! Body operations of the 3D Model tab that change faces or bodies directly
//! (`docs/spec/2.14-thicken-offset.md`; later Delete Face, Move Bodies, Copy
//! Object, Replace Face, the surface commands). One feature type,
//! [`Feature::BodyOp`](super::Feature::BodyOp), like the sheet metal
//! operations: a new operation is a new [`BodyOp`] variant (and a format
//! revision), not a new `Feature` variant.
//!
//! **Thicken/Offset**: picked faces of a solid (or whole surface bodies,
//! *Quilt*) get a thickness — a slab joined to, cut from, intersected with
//! the solid, or a new solid — or are offset into a surface body. Faces are
//! [`FaceRef`]s (persistent name + a point), bodies persistent names.
//! New faces: `<id>:Offset:<face>` (the moved copy of a face), walls
//! `<id>:Wall:<k>`; the face's own copy keeps the face's name.

use serde::{Deserialize, Serialize};

use super::boolean::Operation;
use super::expr::Unit;
use super::shell::FaceRef;
use crate::kernel::Real;

/// What Thicken/Offset takes (Inventor's Select: Face · Quilt).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThickenSelect {
    /// Faces of solids (or of surface bodies).
    #[default]
    Face,
    /// Whole surface bodies.
    Quilt,
}

/// Solid (a thickness) or Surface (an offset copy).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThickenOutput {
    /// A thickness: a slab joined, cut, intersected or a new solid.
    #[default]
    Solid,
    /// An offset copy of the faces (a surface body).
    Surface,
}

/// Which way the thickness or offset goes from the faces.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThickenDirection {
    /// Along the faces' outward normals.
    #[default]
    Direction1,
    /// Against them (into the solid).
    Direction2,
    /// Half each way.
    Symmetric,
}

/// Thicken/Offset.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ThickenOp {
    /// Face or Quilt.
    #[serde(default)]
    pub select: ThickenSelect,
    /// The faces (Face).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub faces: Vec<FaceRef>,
    /// The surface bodies (Quilt), by persistent name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub quilts: Vec<String>,
    /// The solid the result is combined with (multi-body parts); empty: the
    /// solid holding the faces.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub solids: Vec<String>,
    /// Solid or Surface.
    #[serde(default)]
    pub output: ThickenOutput,
    /// Join / Cut / Intersect / New Solid (Output Solid).
    pub operation: Operation,
    /// Thickness or offset (mm, > 0).
    pub distance: Real,
    /// Direction 1, Direction 2 or Symmetric.
    #[serde(default)]
    pub direction: ThickenDirection,
    /// Faces tangent to a picked one come with it.
    #[serde(default = "yes")]
    pub automatic_face_chain: bool,
    /// Create Vertical Surfaces (Output Surface): not built yet.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub vertical_surfaces: bool,
    /// More › Automatic Blending (kept; the kernel blends smooth faces).
    #[serde(default = "yes")]
    pub automatic_blending: bool,
    /// More › Allow Approximation, Specify Tolerance (mm): how far a fitted
    /// free-form face may stray from the true offset. `None`: Optimized (the
    /// finest of 0.01 … 1 mm that fits, at most 2 % of the distance).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<Real>,
}

fn yes() -> bool {
    true
}

impl ThickenOp {
    /// Inventor's defaults: Face, Solid, Join, 1 mm, Direction 1, chains on.
    pub fn new() -> Self {
        Self {
            select: ThickenSelect::Face,
            faces: Vec::new(),
            quilts: Vec::new(),
            solids: Vec::new(),
            output: ThickenOutput::Solid,
            operation: Operation::Join,
            distance: 1.0,
            direction: ThickenDirection::Direction1,
            automatic_face_chain: true,
            vertical_surfaces: false,
            automatic_blending: true,
            tolerance: None,
        }
    }

    /// The offsets of the result's two sides from the faces (`lo`, `hi`),
    /// and whether it is symmetric.
    pub fn signed(&self) -> (Real, bool) {
        match self.direction {
            ThickenDirection::Direction1 => (self.distance, false),
            ThickenDirection::Direction2 => (-self.distance, false),
            ThickenDirection::Symmetric => (self.distance, true),
        }
    }

    /// Checks the inputs before any geometry work.
    pub fn validate(&self) -> Result<(), String> {
        if !(self.distance.is_finite() && self.distance > 0.0) {
            return Err("the distance must be greater than 0".into());
        }
        let picked: Result<(), String> = match self.select {
            ThickenSelect::Face if self.faces.is_empty() => Err("select the faces".into()),
            ThickenSelect::Quilt if self.quilts.is_empty() => {
                Err("select the surface bodies (quilts)".into())
            }
            _ => Ok(()),
        };
        picked?;
        if self
            .tolerance
            .is_some_and(|t| !(t.is_finite() && (1e-6..=10.0).contains(&t)))
        {
            return Err("the tolerance must be 0.000001 … 10 mm".into());
        }
        if self.vertical_surfaces {
            return Err("Create Vertical Surfaces is not supported yet".into());
        }
        if self.output == ThickenOutput::Surface && self.direction == ThickenDirection::Symmetric {
            return Err("an offset surface goes one way (Direction 1 or 2)".into());
        }
        if self.output == ThickenOutput::Surface && self.select == ThickenSelect::Quilt {
            return Err("an offset surface is made from faces (Select: Face)".into());
        }
        Ok(())
    }
}

impl Default for ThickenOp {
    fn default() -> Self {
        Self::new()
    }
}

/// The body operations (one feature type).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum BodyOp {
    /// Thicken/Offset.
    Thicken(ThickenOp),
}

/// A body operation feature (`Thicken1`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BodyOpFeature {
    /// Immutable id (the new faces are named after it).
    pub id: String,
    /// Display name.
    pub name: String,
    /// What it does.
    pub op: BodyOp,
    /// Equations of the fields (`distance`).
    #[serde(
        default,
        skip_serializing_if = "super::params::FeatureParams::is_empty"
    )]
    pub params: super::params::FeatureParams,
}

impl BodyOpFeature {
    /// A feature of this kind (its id is given by `add_body_op`).
    pub fn new(op: BodyOp) -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            op,
            params: Default::default(),
        }
    }

    /// The command's name: the id's prefix and the browser's name.
    pub fn kind_name(&self) -> &'static str {
        match self.op {
            BodyOp::Thicken(_) => "Thicken",
        }
    }

    /// Whether it makes a new body (New Solid, or an offset surface).
    pub fn creates_body(&self) -> bool {
        match &self.op {
            BodyOp::Thicken(t) => {
                t.output == ThickenOutput::Surface || t.operation == Operation::NewSolid
            }
        }
    }

    /// The numeric fields (parameters `d<N>` of the feature).
    pub fn fields(&self) -> Vec<(&'static str, Unit)> {
        match self.op {
            BodyOp::Thicken(_) => vec![("distance", Unit::Mm)],
        }
    }

    /// A field's value.
    pub fn value(&self, field: &str) -> Option<Real> {
        match (&self.op, field) {
            (BodyOp::Thicken(t), "distance") => Some(t.distance),
            _ => None,
        }
    }

    /// Sets a field.
    pub fn set(&mut self, field: &str, v: Real) {
        if let (BodyOp::Thicken(t), "distance") = (&mut self.op, field) {
            t.distance = v;
        }
    }

    /// The browser's tooltip.
    pub fn describe(&self) -> String {
        match &self.op {
            BodyOp::Thicken(t) => {
                let what = match t.select {
                    ThickenSelect::Face => format!("{} face(s)", t.faces.len()),
                    ThickenSelect::Quilt => format!("{} quilt(s)", t.quilts.len()),
                };
                match t.output {
                    ThickenOutput::Surface => format!("Offset of {what} by {} mm", t.distance),
                    ThickenOutput::Solid => {
                        format!("Thicken {what} by {} mm ({:?})", t.distance, t.operation)
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::part::{Feature, PartDocument};

    fn volume(p: &PartDocument) -> Real {
        p.exact_bodies
            .0
            .iter()
            .filter(|s| !s.is_open())
            .filter_map(|s| s.volume_at(1e-3))
            .sum()
    }

    fn top(part: &PartDocument) -> FaceRef {
        let _ = part;
        FaceRef {
            face: "Box1:Top".into(),
            hint: [20.0, 15.0, 20.0],
        }
    }

    /// A 40 × 30 × 20 box and Thicken/Offset of its top face.
    fn thicken(f: impl FnOnce(&mut ThickenOp)) -> Result<PartDocument, String> {
        let mut part =
            PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).map_err(|e| e.to_string())?;
        let mut t = ThickenOp::new();
        t.faces = vec![top(&part)];
        f(&mut t);
        part.add_body_op(BodyOpFeature::new(BodyOp::Thicken(t)))
            .map_err(|e| e.to_string())?;
        Ok(part)
    }

    fn close(a: Real, b: Real) -> bool {
        (a - b).abs() < 1e-3 * b.abs().max(1.0)
    }

    /// Join, Cut, Intersect, Symmetric and New Solid give the volumes of
    /// the slab on the top face.
    #[test]
    fn operations_and_directions() {
        let cases: [(Operation, ThickenDirection, Real, Real, usize); 5] = [
            (
                Operation::Join,
                ThickenDirection::Direction1,
                5.0,
                40.0 * 30.0 * 25.0,
                1,
            ),
            (
                Operation::Cut,
                ThickenDirection::Direction2,
                5.0,
                40.0 * 30.0 * 15.0,
                1,
            ),
            (
                Operation::Intersect,
                ThickenDirection::Direction2,
                5.0,
                40.0 * 30.0 * 5.0,
                1,
            ),
            (
                Operation::Join,
                ThickenDirection::Symmetric,
                4.0,
                40.0 * 30.0 * 22.0,
                1,
            ),
            (
                Operation::NewSolid,
                ThickenDirection::Direction1,
                5.0,
                40.0 * 30.0 * 25.0,
                2,
            ),
        ];
        for (op, dir, d, want, solids) in cases {
            let part = thicken(|t| {
                t.operation = op;
                t.direction = dir;
                t.distance = d;
            })
            .unwrap_or_else(|e| panic!("{op:?} {dir:?}: {e}"));
            assert!(part.feature_health.is_empty(), "{:?}", part.feature_health);
            let v = volume(&part);
            assert!(close(v, want), "{op:?} {dir:?}: volume {v}, want {want}");
            let n = part.exact_bodies.0.iter().filter(|s| !s.is_open()).count();
            assert_eq!(n, solids, "{op:?}");
        }
    }

    /// The moved copy of the face is named after the feature; walls flush
    /// with the sides merge into them (the sides keep their names); the
    /// solid keeps its name. A New Solid shows its own walls.
    #[test]
    fn names() {
        let part = thicken(|t| t.distance = 5.0).unwrap_or_else(|e| panic!("{e}"));
        let names: Vec<String> = part
            .body
            .faces()
            .filter_map(|(_, f)| f.name.clone())
            .collect();
        assert!(
            names.iter().any(|n| n == "Thicken1:Offset:Box1:Top"),
            "{names:?}"
        );
        assert!(names.iter().any(|n| n == "Box1:Front"), "{names:?}");
        assert!(
            part.body
                .solids()
                .any(|(_, s)| s.name.as_deref() == Some("Box1"))
        );
        let apart =
            thicken(|t| t.operation = Operation::NewSolid).unwrap_or_else(|e| panic!("{e}"));
        let names: Vec<String> = apart
            .body
            .faces()
            .filter_map(|(_, f)| f.name.clone())
            .collect();
        assert!(
            names.iter().any(|n| n.starts_with("Thicken1:Wall:")),
            "{names:?}"
        );
        assert!(
            apart
                .body
                .solids()
                .any(|(_, s)| s.name.as_deref() == Some("Thicken1"))
        );
    }

    /// Output Surface: a surface body 3 mm above the top; the solid is
    /// unchanged. A second Thicken (Quilt) makes it a 2 mm plate.
    #[test]
    fn offset_surface_and_quilt() {
        let mut part = thicken(|t| {
            t.output = ThickenOutput::Surface;
            t.distance = 3.0;
        })
        .unwrap_or_else(|e| panic!("{e}"));
        assert!(close(volume(&part), 40.0 * 30.0 * 20.0));
        assert_eq!(
            part.exact_bodies.0.iter().filter(|s| s.is_open()).count(),
            1
        );
        let mut q = ThickenOp::new();
        q.select = ThickenSelect::Quilt;
        q.quilts = vec!["Thicken1".into()];
        q.operation = Operation::NewSolid;
        q.distance = 2.0;
        part.add_body_op(BodyOpFeature::new(BodyOp::Thicken(q)))
            .unwrap_or_else(|e| panic!("{e}"));
        let v = volume(&part);
        assert!(close(v, 40.0 * 30.0 * 22.0), "volume {v}");
    }

    /// Two faces at once (top and front), and the distance as a parameter.
    #[test]
    fn several_faces_and_the_parameter() {
        let mut part = thicken(|t| {
            t.faces.push(FaceRef {
                face: "Box1:Front".into(),
                hint: [20.0, 0.0, 10.0],
            });
            t.distance = 2.0;
            t.automatic_face_chain = false;
        })
        .unwrap_or_else(|e| panic!("{e}"));
        assert!(volume(&part) > 40.0 * 30.0 * 20.0 + 40.0 * 30.0 * 2.0);
        let d = part
            .parameter_table()
            .into_iter()
            .find(|r| r.consumed_by.contains("Thicken1"))
            .map(|r| r.name)
            .unwrap_or_default();
        assert!(!d.is_empty(), "no distance parameter");
        part.set_parameter_expr(&d, "4 mm")
            .unwrap_or_else(|e| panic!("{e}"));
        part.regenerate().unwrap_or_else(|e| panic!("{e}"));
        assert!(volume(&part) > 40.0 * 30.0 * 20.0 + 40.0 * 30.0 * 4.0);
    }

    /// Refusals change nothing: no face, a missing face, Surface with
    /// Symmetric, distance 0.
    #[test]
    fn refusals_leave_the_part() {
        let mut part =
            PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap_or_else(|e| panic!("{e}"));
        let before = part.features.len();
        let bad = [
            ThickenOp::new(),
            ThickenOp {
                faces: vec![FaceRef {
                    face: "Nope:Top".into(),
                    hint: [0.0; 3],
                }],
                ..ThickenOp::new()
            },
            ThickenOp {
                faces: vec![top(&part)],
                output: ThickenOutput::Surface,
                direction: ThickenDirection::Symmetric,
                ..ThickenOp::new()
            },
            ThickenOp {
                faces: vec![top(&part)],
                distance: 0.0,
                ..ThickenOp::new()
            },
        ];
        for t in bad {
            assert!(
                part.add_body_op(BodyOpFeature::new(BodyOp::Thicken(t.clone())))
                    .is_err(),
                "{t:?}"
            );
            assert_eq!(part.features.len(), before);
        }
        assert!(close(volume(&part), 40.0 * 30.0 * 20.0));
    }

    /// Save and load: the same feature and volume.
    #[test]
    fn save_and_load() {
        let part = thicken(|t| t.distance = 5.0).unwrap_or_else(|e| panic!("{e}"));
        let json = serde_json::to_string(&part).unwrap_or_default();
        let mut back: PartDocument = serde_json::from_str(&json).unwrap_or_else(|e| panic!("{e}"));
        back.regenerate().unwrap_or_else(|e| panic!("{e}"));
        assert!(matches!(back.features.last(), Some(Feature::BodyOp(_))));
        assert!(close(volume(&back), volume(&part)));
    }
}
