//! **Face Draft** (Inventor: 3D Model → Modify → Draft,
//! `docs/spec/2.8-draft.md`): tilts faces by an angle against a pull
//! direction, about a fixed edge of each face or a fixed plane. The drafted
//! faces keep their names.

use serde::{Deserialize, Serialize};

use crate::kernel::{BRep, FaceId, Plane, Point3, Real, TessellationOptions, Vector3};

use super::params::FeatureParams;
use super::placement::{PlaneRef, WorkGeom, plane_of};
use super::shell::FaceRef;
use super::work::LineRef;

/// What the faces pivot about.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DraftType {
    /// Each face about its edge nearest the pick (One Way).
    #[default]
    FixedEdge,
    /// Every face about its line on a plane; the pull is the plane's normal.
    FixedPlane,
}

impl DraftType {
    /// The type's name as Inventor shows it.
    pub fn label(self) -> &'static str {
        match self {
            DraftType::FixedEdge => "Fixed Edge",
            DraftType::FixedPlane => "Fixed Plane",
        }
    }
}

/// A pull direction: a plane's normal or a straight line's direction.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PullRef {
    /// The normal of a planar face, a work plane or an origin plane.
    Plane(PlaneRef),
    /// A straight edge, a work axis or an origin axis.
    Line(LineRef),
}

fn yes() -> bool {
    true
}

/// The Face Draft feature.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DraftFeature {
    /// Immutable identity.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Fixed Edge or Fixed Plane.
    #[serde(default)]
    pub draft_type: DraftType,
    /// The pull direction (Fixed Edge); Fixed Plane uses its plane's normal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pull: Option<PullRef>,
    /// Reverses the pull direction.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub flip: bool,
    /// The fixed plane (Fixed Plane).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plane: Option<PlaneRef>,
    /// Faces to draft; with Fixed Edge the hint is where the face was
    /// clicked (its nearest edge stays fixed).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub faces: Vec<FaceRef>,
    /// Draft angle (radians); parameter field `angle`.
    pub angle: Real,
    /// Automatic Face Chain (applied when picking).
    #[serde(default = "yes")]
    pub chain: bool,
    /// Equations of its values.
    #[serde(default, skip_serializing_if = "FeatureParams::is_empty")]
    pub params: FeatureParams,
}

impl DraftFeature {
    /// A draft with Inventor's defaults (Fixed Edge, chain on), no inputs.
    pub fn new(angle: Real) -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            draft_type: DraftType::FixedEdge,
            pull: None,
            flip: false,
            plane: None,
            faces: Vec::new(),
            angle,
            chain: true,
            params: FeatureParams::default(),
        }
    }

    /// Picks face `f` clicked at `at`: adds it (with its tangent chain), or
    /// removes it when it is already in the list.
    pub fn toggle(&mut self, body: &BRep, f: FaceId, at: Option<Point3>) {
        let Some(r) = super::shell::face_ref(body, f, at) else {
            return;
        };
        if self.faces.iter().any(|x| x.face == r.face) {
            let chain: Vec<String> = if self.chain {
                super::select::tangent_faces(body, f)
                    .into_iter()
                    .filter_map(|g| body.face(g).and_then(|x| x.name.clone()))
                    .collect()
            } else {
                vec![r.face.clone()]
            };
            self.faces.retain(|x| !chain.contains(&x.face));
            return;
        }
        let refs: Vec<FaceRef> = if self.chain {
            super::select::tangent_faces(body, f)
                .into_iter()
                .filter_map(|g| super::shell::face_ref(body, g, (g == f).then_some(at).flatten()))
                .collect()
        } else {
            vec![r]
        };
        for y in refs {
            if !self.faces.iter().any(|x| x.face == y.face) {
                self.faces.push(y);
            }
        }
    }

    /// The unit pull direction (flip applied).
    pub fn pull_direction(&self, body: &BRep, works: &WorkGeom) -> Result<Vector3, String> {
        let d = match (self.draft_type, &self.pull, &self.plane) {
            (DraftType::FixedPlane, _, Some(p)) => plane_of(p, body, works)
                .map_err(|e| e.to_string())?
                .normal
                .into_inner(),
            (DraftType::FixedPlane, _, None) => return Err("select the fixed plane".into()),
            (DraftType::FixedEdge, Some(PullRef::Plane(p)), _) => plane_of(p, body, works)
                .map_err(|e| e.to_string())?
                .normal
                .into_inner(),
            (DraftType::FixedEdge, Some(PullRef::Line(l)), _) => l.resolve(body, works)?.1,
            (DraftType::FixedEdge, None, _) => return Err("select the pull direction".into()),
        };
        let d = d
            .try_normalize(1e-12)
            .ok_or_else(|| "the pull direction has no length".to_owned())?;
        Ok(if self.flip { -d } else { d })
    }

    /// The neutral plane of each face (in order of `faces`): the fixed
    /// plane, or (Fixed Edge) the plane square to `pull` through the face's
    /// edge nearest its hint.
    pub fn neutral_planes(
        &self,
        body: &BRep,
        works: &WorkGeom,
        pull: Vector3,
    ) -> Result<Vec<Plane>, String> {
        if self.draft_type == DraftType::FixedPlane {
            let p = self
                .plane
                .as_ref()
                .ok_or_else(|| "select the fixed plane".to_owned())?;
            let pl = plane_of(p, body, works).map_err(|e| e.to_string())?;
            return Ok(vec![pl; self.faces.len()]);
        }
        self.faces
            .iter()
            .map(|r| {
                let f = super::shell::face_near(body, &r.face, Point3::from(r.hint))
                    .ok_or_else(|| format!("the face {} no longer exists", r.face))?;
                fixed_edge_plane(body, f, Point3::from(r.hint), pull).ok_or_else(|| {
                    format!(
                        "the fixed edge of {} is not square to the pull direction (not supported yet)",
                        r.face
                    )
                })
            })
            .collect()
    }
}

/// The plane square to `pull` through the edge of `f` nearest `hint`, if
/// that edge lies in such a plane.
fn fixed_edge_plane(body: &BRep, f: FaceId, hint: Point3, pull: Vector3) -> Option<Plane> {
    let opts = TessellationOptions::default();
    let size = body
        .vertices()
        .map(|(_, v)| v.point.coords.norm())
        .fold(1.0, Real::max);
    let mut best: Option<(Real, Point3)> = None;
    for e in super::select::edges_of_face(body, f) {
        let Ok(pts) = body.sample_edge(e, &opts) else {
            continue;
        };
        let Some(first) = pts.first() else { continue };
        let h0 = first.coords.dot(&pull);
        if pts
            .iter()
            .any(|p| (p.coords.dot(&pull) - h0).abs() > 1e-7 * size)
        {
            continue;
        }
        let d = pts
            .windows(2)
            .map(|w| {
                let (a, b) = (w[0], w[1]);
                let ab = b - a;
                let l2 = ab.norm_squared();
                let t = if l2 > 0.0 {
                    ((hint - a).dot(&ab) / l2).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                (a + ab * t - hint).norm()
            })
            .fold(Real::INFINITY, Real::min);
        if best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, *first));
        }
    }
    let (_, p) = best?;
    Plane::from_normal(p, pull).ok()
}
