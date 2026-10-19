//! **Shell** (Inventor: 3D Model → Modify → Shell, `docs/spec/2.6-shell.md`):
//! hollows a solid with walls of one thickness, removing the chosen faces
//! (an open box) or none (a closed cavity). Walls grow Inside, Outside or
//! Both ways from the faces.
//!
//! Faces are referenced by persistent name plus a point on them when picked
//! ([`FaceRef`]); inner walls are named `<id>:Inner[<face they line>]`.

use serde::{Deserialize, Serialize};

use crate::kernel::{BRep, FaceId, Point3, Real};

use super::params::FeatureParams;

/// A model face: its persistent name and a point on it when it was picked
/// (chooses among pieces a later Boolean split it into).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FaceRef {
    /// Persistent face name (`Box1:Top`, `Extrusion1:End`).
    pub face: String,
    /// A point on the face (model coordinates).
    pub hint: [Real; 3],
}

/// Which way the walls grow from the faces.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShellDirection {
    /// Into the material: the part keeps its outside size.
    #[default]
    Inside,
    /// Out of it: the old faces become the inner walls.
    Outside,
    /// Half the thickness each way (Inventor 2027: Center; saved as
    /// `Both`, the name of older builds).
    #[serde(rename = "Both", alias = "Center")]
    Center,
}

impl ShellDirection {
    /// The direction's name as Inventor shows it.
    pub fn label(self) -> &'static str {
        match self {
            ShellDirection::Inside => "Inside",
            ShellDirection::Outside => "Outside",
            ShellDirection::Center => "Center",
        }
    }
}

/// Shell type (Inventor 2026+): how the corners the offset makes look.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShellCorners {
    /// Sharp offset corners.
    #[default]
    Sharp,
    /// The corners the offset makes are rounded (by the offset distance).
    Rounded,
}

impl ShellCorners {
    /// The type's name as Inventor shows it.
    pub fn label(self) -> &'static str {
        match self {
            ShellCorners::Sharp => "Sharp Corners",
            ShellCorners::Rounded => "Rounded Corners",
        }
    }
}

fn yes() -> bool {
    true
}

fn is_sharp(c: &ShellCorners) -> bool {
    *c == ShellCorners::Sharp
}

/// The Shell feature.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShellFeature {
    /// Immutable identity (topology-name prefix).
    pub id: String,
    /// Display name.
    pub name: String,
    /// Faces to remove (Inventor's Remove Faces); empty: a closed cavity.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remove: Vec<FaceRef>,
    /// Wall thickness (mm); parameter field `thickness`.
    pub thickness: Real,
    /// Inside / Outside / Center.
    #[serde(default)]
    pub direction: ShellDirection,
    /// Sharp or Rounded Corners (format revision 9).
    #[serde(default, skip_serializing_if = "is_sharp")]
    pub corners: ShellCorners,
    /// Automatic Face Chain: a picked face brings the faces tangent to it
    /// (applied when picking; the list holds every removed face).
    #[serde(default = "yes")]
    pub chain: bool,
    /// The solid to shell (persistent body name). Empty: the solid of the
    /// removed faces, or the only one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub solids: Vec<String>,
    /// Equations of its values.
    #[serde(default, skip_serializing_if = "FeatureParams::is_empty")]
    pub params: FeatureParams,
    /// The preset the feature was made with (`docs/spec/0.11-presets.md`):
    /// only its name, Edit Feature shows it again; the values are the
    /// feature's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
}

impl ShellFeature {
    /// A shell with Inventor's defaults (Inside, chain on), no faces yet.
    pub fn new(thickness: Real) -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            remove: Vec::new(),
            thickness,
            direction: ShellDirection::Inside,
            corners: ShellCorners::Sharp,
            chain: true,
            solids: Vec::new(),
            params: FeatureParams::default(),
            preset: None,
        }
    }

    /// Whether `r` (by name, nearest piece) is among the removed faces.
    pub fn removes(&self, r: &FaceRef) -> bool {
        self.remove.iter().any(|x| x.face == r.face)
    }

    /// Picks face `f` (clicked at `at`): adds it with its tangent chain, or
    /// removes the chain when the face is already in the list (Inventor: a
    /// second click deselects).
    pub fn toggle(&mut self, body: &BRep, f: FaceId, at: Option<Point3>) {
        let Some(r) = face_ref(body, f, at) else {
            return;
        };
        let refs: Vec<FaceRef> = if self.chain {
            super::select::tangent_faces(body, f)
                .into_iter()
                .filter_map(|g| face_ref(body, g, (g == f).then_some(at).flatten()))
                .collect()
        } else {
            vec![r.clone()]
        };
        if self.removes(&r) {
            self.remove
                .retain(|x| !refs.iter().any(|y| y.face == x.face));
        } else {
            for y in refs {
                if !self.removes(&y) {
                    self.remove.push(y);
                }
            }
        }
    }
}

/// The reference of face `f`: its name and `at` (or a point of its
/// triangles) as the hint.
pub fn face_ref(body: &BRep, f: FaceId, at: Option<Point3>) -> Option<FaceRef> {
    let face = body.face(f)?;
    let name = face.name.clone()?;
    let p = at.or_else(|| {
        let m = face.mesh.as_ref()?;
        let t = m.triangles.first()?;
        let [a, b, c] = t.map(|i| m.positions.get(i as usize).copied());
        Some(Point3::from((a?.coords + b?.coords + c?.coords) / 3.0))
    })?;
    Some(FaceRef {
        face: name,
        hint: [p.x, p.y, p.z],
    })
}

/// The face named `name` nearest `hint` (a face a later Boolean split keeps
/// its name on each piece).
pub fn face_near(body: &BRep, name: &str, hint: Point3) -> Option<FaceId> {
    body.faces()
        .filter(|(_, f)| f.name.as_deref() == Some(name))
        .map(|(id, f)| {
            let d = f
                .mesh
                .as_ref()
                .and_then(|m| {
                    m.positions
                        .iter()
                        .map(|p| (p - hint).norm())
                        .min_by(|a, b| a.total_cmp(b))
                })
                .unwrap_or(Real::INFINITY);
            (id, d)
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}
