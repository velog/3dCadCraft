//! **Face Draft** (Inventor: 3D Model → Modify → Draft,
//! `docs/spec/2.8-draft.md`): tilts faces by an angle against a pull
//! direction, about a fixed edge of each face, a fixed plane or a parting
//! line; one way, or both ways from the plane (Symmetric / Asymmetric). The
//! drafted faces keep their names (a face split by the plane: both pieces).

use serde::{Deserialize, Serialize};

use crate::kernel::{BRep, EdgeId, FaceId, Plane, Point3, Real, TessellationOptions, Vector3};

use super::blend::EdgeRef;
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
    /// About a parting tool (a plane or a sketch line), with its own pull
    /// direction; the faces are drafted above and below it (format
    /// revision 9).
    PartingLine,
}

impl DraftType {
    /// The type's name as Inventor shows it.
    pub fn label(self) -> &'static str {
        match self {
            DraftType::FixedEdge => "Fixed Edge",
            DraftType::FixedPlane => "Fixed Plane",
            DraftType::PartingLine => "Parting Line",
        }
    }

    /// Whether the type takes a Pull Direction selection (Fixed Plane uses
    /// its plane's normal).
    pub fn has_pull(self) -> bool {
        self != DraftType::FixedPlane
    }

    /// The extents the type allows (Inventor: One Way for Fixed Edge and
    /// Fixed Plane; Symmetric and Asymmetric for Fixed Plane and Parting
    /// Line).
    pub fn extents(self) -> &'static [DraftExtent] {
        match self {
            DraftType::FixedEdge => &[DraftExtent::OneWay],
            DraftType::FixedPlane => &[
                DraftExtent::OneWay,
                DraftExtent::Symmetric,
                DraftExtent::Asymmetric,
            ],
            DraftType::PartingLine => &[DraftExtent::Symmetric, DraftExtent::Asymmetric],
        }
    }
}

/// Which sides of the plane are drafted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DraftExtent {
    /// The whole face, in the pull direction.
    #[default]
    OneWay,
    /// Above and below the plane, the same angle, each away from it.
    Symmetric,
    /// Above (Draft Angle) and below (Draft Angle 2) the plane.
    Asymmetric,
}

impl DraftExtent {
    /// The option's name as Inventor shows it.
    pub fn label(self) -> &'static str {
        match self {
            DraftExtent::OneWay => "One Way",
            DraftExtent::Symmetric => "Symmetric",
            DraftExtent::Asymmetric => "Asymmetric",
        }
    }
}

/// The parting tool of a Parting Line draft.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PartingTool {
    /// A planar face, a work plane or an origin plane.
    Plane(PlaneRef),
    /// A 2D sketch (by id) whose lines give the parting line.
    Sketch(String),
}

/// With Move Parting Line: which side of the parting line is drafted about
/// its static edges (Inventor: Angle for Top / Both / Bottom).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PartingSide {
    /// The part above the parting line (along the pull).
    #[default]
    Top,
    /// Both parts, each about its own static edges.
    Both,
    /// The part below it.
    Bottom,
}

impl PartingSide {
    /// The option's name as Inventor shows it.
    pub fn label(self) -> &'static str {
        match self {
            PartingSide::Top => "Angle for Top",
            PartingSide::Both => "Angle for Both",
            PartingSide::Bottom => "Angle for Bottom",
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

fn default_angle2() -> Real {
    1f64.to_radians()
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

/// One side of a two-sided draft: its neutral plane and angle.
#[derive(Clone, Debug, PartialEq)]
pub struct DraftSide {
    /// The plane the faces pivot about (square to the pull).
    pub neutral: Plane,
    /// The draft angle (radians).
    pub angle: Real,
}

/// A two-sided draft, ready for the kernel: split by `plane`, the faces'
/// pieces above it drafted by `above` (leaning in along `pull`), those
/// below by `below` (leaning in along `−pull`).
#[derive(Clone, Debug, PartialEq)]
pub struct TwoSided {
    /// The parting plane.
    pub plane: Plane,
    /// Above the plane (`None`: left as it is).
    pub above: Option<DraftSide>,
    /// Below it.
    pub below: Option<DraftSide>,
}

/// The Face Draft feature.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DraftFeature {
    /// Immutable identity.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Fixed Edge, Fixed Plane or Parting Line.
    #[serde(default)]
    pub draft_type: DraftType,
    /// The pull direction (Fixed Edge, Parting Line); Fixed Plane uses its
    /// plane's normal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pull: Option<PullRef>,
    /// Reverses the pull direction.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub flip: bool,
    /// The fixed plane (Fixed Plane).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plane: Option<PlaneRef>,
    /// The parting tool (Parting Line).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parting: Option<PartingTool>,
    /// Faces to draft; with Fixed Edge the hint is where the face was
    /// clicked (its nearest edge stays fixed).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub faces: Vec<FaceRef>,
    /// Draft angle (radians); parameter field `angle`. Two-sided: the
    /// angle above the plane.
    pub angle: Real,
    /// One Way, Symmetric or Asymmetric.
    #[serde(default, skip_serializing_if = "is_default")]
    pub extent: DraftExtent,
    /// Draft Angle 2 (Asymmetric: below the plane); parameter `angle2`.
    #[serde(default = "default_angle2")]
    pub angle2: Real,
    /// Automatic Face Chain (applied when picking).
    #[serde(default = "yes")]
    pub chain: bool,
    /// Automatic Blending: rounds along the pull next to a drafted face
    /// are drafted with it, so they stay tangent.
    #[serde(default = "yes")]
    pub blending: bool,
    /// Move Parting Line (Parting Line): the faces pivot about their
    /// static edges instead of the parting line, which moves.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub move_parting: bool,
    /// The static edges (Move Parting Line).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub static_edges: Vec<EdgeRef>,
    /// Angle for Top / Both / Bottom (Move Parting Line).
    #[serde(default, skip_serializing_if = "is_default")]
    pub move_side: PartingSide,
    /// Equations of its values.
    #[serde(default, skip_serializing_if = "FeatureParams::is_empty")]
    pub params: FeatureParams,
    /// The preset the feature was made with (`docs/spec/0.11-presets.md`):
    /// only its name, Edit Feature shows it again; the values are the
    /// feature's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
}

impl DraftFeature {
    /// A draft with Inventor's defaults (Fixed Edge, One Way, chain and
    /// blending on), no inputs.
    pub fn new(angle: Real) -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            draft_type: DraftType::FixedEdge,
            pull: None,
            flip: false,
            plane: None,
            parting: None,
            faces: Vec::new(),
            angle,
            extent: DraftExtent::OneWay,
            angle2: default_angle2(),
            chain: true,
            blending: true,
            move_parting: false,
            static_edges: Vec::new(),
            move_side: PartingSide::Top,
            params: FeatureParams::default(),
            preset: None,
        }
    }

    /// Changes the type and keeps the extent one the type allows.
    pub fn set_type(&mut self, t: DraftType) {
        self.draft_type = t;
        if !t.extents().contains(&self.extent) {
            self.extent = t.extents()[0];
        }
        if t != DraftType::PartingLine {
            self.move_parting = false;
        }
    }

    /// The extent in effect (one the type allows).
    pub fn effective_extent(&self) -> DraftExtent {
        if self.draft_type.extents().contains(&self.extent) {
            self.extent
        } else {
            self.draft_type.extents()[0]
        }
    }

    /// Whether Draft Angle 2 is used (Asymmetric, or Move Parting Line for
    /// both sides).
    pub fn uses_angle2(&self) -> bool {
        self.effective_extent() == DraftExtent::Asymmetric
    }

    /// The parting sketch's id (Parting Line with a sketch).
    pub fn parting_sketch(&self) -> Option<&str> {
        match (&self.draft_type, &self.parting) {
            (DraftType::PartingLine, Some(PartingTool::Sketch(id))) => Some(id),
            _ => None,
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

    /// Picks a static edge (Move Parting Line): adds it, or removes it when
    /// it is already in the list.
    pub fn toggle_static(&mut self, body: &BRep, e: EdgeId) {
        let Some(r) = super::blend::edge_ref(body, e) else {
            return;
        };
        if let Some(k) = self
            .static_edges
            .iter()
            .position(|x| edge_of(body, x) == Some(e))
        {
            self.static_edges.remove(k);
        } else {
            self.static_edges.push(r);
        }
    }

    /// Select Boundary (Move Parting Line): the edges of the faces that are
    /// square to the pull at the top of each face (Top), its bottom
    /// (Bottom) or both, replacing the static edges.
    pub fn select_boundary(
        &mut self,
        body: &BRep,
        works: &WorkGeom,
        sketch_lines: Option<&[(Point3, Point3, Vector3)]>,
    ) -> Result<usize, String> {
        let pull = self.pull_direction(body, works)?;
        let mid = self
            .parting_plane(body, works, pull, sketch_lines)
            .map(|p| p.origin.coords.dot(&pull))
            .ok();
        let opts = TessellationOptions::default();
        let mut out: Vec<EdgeRef> = Vec::new();
        for r in &self.faces {
            let Some(f) = super::shell::face_near(body, &r.face, Point3::from(r.hint)) else {
                continue;
            };
            // Level edges with their height.
            let level: Vec<(EdgeId, Real)> = super::select::edges_of_face(body, f)
                .into_iter()
                .filter_map(|e| {
                    let pts = body.sample_edge(e, &opts).ok()?;
                    let h = pts.first()?.coords.dot(&pull);
                    pts.iter()
                        .all(|p| (p.coords.dot(&pull) - h).abs() < 1e-6)
                        .then_some((e, h))
                })
                .collect();
            let (lo, hi) = level
                .iter()
                .fold((Real::INFINITY, Real::NEG_INFINITY), |(a, b), (_, h)| {
                    (a.min(*h), b.max(*h))
                });
            for (e, h) in level {
                let top = (h - hi).abs() < 1e-6 && mid.is_none_or(|m| h > m);
                let bottom = (h - lo).abs() < 1e-6 && mid.is_none_or(|m| h < m);
                let take = match self.move_side {
                    PartingSide::Top => top,
                    PartingSide::Bottom => bottom,
                    PartingSide::Both => top || bottom,
                };
                if take
                    && let Some(er) = super::blend::edge_ref(body, e)
                    && !out.contains(&er)
                {
                    out.push(er);
                }
            }
        }
        self.static_edges = out;
        Ok(self.static_edges.len())
    }

    /// The faces to draft on `body`: the picked ones and, with Automatic
    /// Blending, the rounds along the pull next to them (tangent there),
    /// so a drafted wall and its round stay tangent.
    pub fn faces_with_blends(&self, body: &BRep, pull: Vector3) -> Vec<FaceRef> {
        let mut out = self.faces.clone();
        if !self.blending {
            return out;
        }
        let along = |g: FaceId| {
            super::blend::cylinder_of(body, g)
                .is_some_and(|c| c.axis.into_inner().dot(&pull).abs() > 1.0 - 1e-9)
        };
        for r in &self.faces {
            let Some(f) = super::shell::face_near(body, &r.face, Point3::from(r.hint)) else {
                continue;
            };
            let chain = super::select::tangent_faces(body, f);
            // From f through rounds only (not on to the next flat wall).
            let mut seen = vec![f];
            let mut stack = vec![f];
            while let Some(a) = stack.pop() {
                for e in super::select::edges_of_face(body, a) {
                    for b in body.edge_faces(e) {
                        if seen.contains(&b) || !chain.contains(&b) || !along(b) {
                            continue;
                        }
                        seen.push(b);
                        stack.push(b);
                        if let Some(y) = super::shell::face_ref(body, b, None)
                            && !out.iter().any(|x| x.face == y.face)
                        {
                            out.push(y);
                        }
                    }
                }
            }
        }
        out
    }

    /// The unit pull direction (flip applied).
    pub fn pull_direction(&self, body: &BRep, works: &WorkGeom) -> Result<Vector3, String> {
        let d = match (self.draft_type, &self.pull, &self.plane) {
            (DraftType::FixedPlane, _, Some(p)) => plane_of(p, body, works)
                .map_err(|e| e.to_string())?
                .normal
                .into_inner(),
            (DraftType::FixedPlane, _, None) => return Err("select the fixed plane".into()),
            (_, Some(PullRef::Plane(p)), _) => plane_of(p, body, works)
                .map_err(|e| e.to_string())?
                .normal
                .into_inner(),
            (_, Some(PullRef::Line(l)), _) => l.resolve(body, works)?.1,
            (_, None, _) => return Err("select the pull direction".into()),
        };
        let d = d
            .try_normalize(1e-12)
            .ok_or_else(|| "the pull direction has no length".to_owned())?;
        Ok(if self.flip { -d } else { d })
    }

    /// The parting plane: the fixed plane (Fixed Plane) or the parting
    /// tool (Parting Line). A sketch gives the plane its lines sweep along
    /// the sketch normal (`sketch_lines`: world ends and the sketch normal);
    /// the plane must be square to the pull.
    pub fn parting_plane(
        &self,
        body: &BRep,
        works: &WorkGeom,
        pull: Vector3,
        sketch_lines: Option<&[(Point3, Point3, Vector3)]>,
    ) -> Result<Plane, String> {
        let pl = match (self.draft_type, &self.plane, &self.parting) {
            (DraftType::FixedPlane, Some(p), _) => {
                plane_of(p, body, works).map_err(|e| e.to_string())?
            }
            (DraftType::FixedPlane, None, _) => return Err("select the fixed plane".into()),
            (DraftType::PartingLine, _, Some(PartingTool::Plane(p))) => {
                plane_of(p, body, works).map_err(|e| e.to_string())?
            }
            (DraftType::PartingLine, _, Some(PartingTool::Sketch(id))) => {
                let lines = sketch_lines
                    .filter(|l| !l.is_empty())
                    .ok_or_else(|| format!("the parting sketch {id} has no lines"))?;
                sketch_parting_plane(lines, pull)?
            }
            (DraftType::PartingLine, _, None) => return Err("select the parting tool".into()),
            (DraftType::FixedEdge, ..) => return Err("a fixed edge draft has no plane".into()),
        };
        if pl.normal.dot(&pull).abs() < 1.0 - 1e-9 {
            return Err(
                "the parting plane is not square to the pull direction (not supported yet)".into(),
            );
        }
        Ok(pl)
    }

    /// The neutral plane of each face (in order of `faces`): the fixed
    /// plane, or (Fixed Edge) the plane square to `pull` through the face's
    /// edge nearest its hint.
    pub fn neutral_planes(
        &self,
        body: &BRep,
        works: &WorkGeom,
        pull: Vector3,
        faces: &[FaceRef],
    ) -> Result<Vec<Plane>, String> {
        if self.draft_type == DraftType::FixedPlane {
            let pl = self.parting_plane(body, works, pull, None)?;
            return Ok(vec![pl; faces.len()]);
        }
        faces
            .iter()
            .map(|r| {
                let f = super::shell::face_near(body, &r.face, Point3::from(r.hint))
                    .ok_or_else(|| format!("the face {} no longer exists", r.face))?;
                fixed_edge(body, f, Point3::from(r.hint), pull)
                    .map(|(_, p)| p)
                    .ok_or_else(|| {
                        format!(
                            "the fixed edge of {} is not square to the pull direction (not supported yet)",
                            r.face
                        )
                    })
            })
            .collect()
    }

    /// The two-sided definition (Fixed Plane Symmetric / Asymmetric,
    /// Parting Line): `None` for a one-way draft.
    pub fn two_sided(
        &self,
        body: &BRep,
        works: &WorkGeom,
        pull: Vector3,
        sketch_lines: Option<&[(Point3, Point3, Vector3)]>,
    ) -> Result<Option<TwoSided>, String> {
        let extent = self.effective_extent();
        if self.draft_type == DraftType::FixedEdge
            || (self.draft_type == DraftType::FixedPlane && extent == DraftExtent::OneWay)
        {
            return Ok(None);
        }
        let plane = self.parting_plane(body, works, pull, sketch_lines)?;
        let a2 = if extent == DraftExtent::Asymmetric {
            self.angle2
        } else {
            self.angle
        };
        if !(a2.is_finite() && a2.abs() < std::f64::consts::FRAC_PI_2 - 1e-3) {
            return Err("the draft angle 2 must be between −90° and 90°".into());
        }
        if !self.move_parting {
            return Ok(Some(TwoSided {
                plane: plane.clone(),
                above: Some(DraftSide {
                    neutral: plane.clone(),
                    angle: self.angle,
                }),
                below: Some(DraftSide {
                    neutral: plane,
                    angle: a2,
                }),
            }));
        }
        // Move Parting Line: each side pivots about its static edges.
        let h0 = plane.origin.coords.dot(&pull);
        let mut above: Option<Real> = None;
        let mut below: Option<Real> = None;
        let opts = TessellationOptions::default();
        for r in &self.static_edges {
            let e = edge_of(body, r).ok_or("a static edge no longer exists")?;
            let pts = body.sample_edge(e, &opts).map_err(|e| e.to_string())?;
            let h = pts
                .first()
                .ok_or("a static edge has no points")?
                .coords
                .dot(&pull);
            if pts.iter().any(|p| (p.coords.dot(&pull) - h).abs() > 1e-6) {
                return Err(
                    "a static edge is not square to the pull direction (not supported yet)".into(),
                );
            }
            let slot = if h > h0 { &mut above } else { &mut below };
            match slot {
                Some(x) if (*x - h).abs() > 1e-6 => {
                    return Err(
                        "the static edges on one side are at different heights (not supported yet)"
                            .into(),
                    );
                }
                _ => *slot = Some(h),
            }
        }
        let side = |h: Option<Real>, angle: Real, what: &str| -> Result<DraftSide, String> {
            let h = h.ok_or_else(|| format!("select the static edges {what} the parting line"))?;
            Ok(DraftSide {
                neutral: Plane::from_normal(Point3::from(pull * h), pull)
                    .map_err(|e| e.to_string())?,
                angle,
            })
        };
        let (above, below) = match self.move_side {
            PartingSide::Top => (Some(side(above, self.angle, "above")?), None),
            PartingSide::Bottom => (None, Some(side(below, self.angle, "below")?)),
            PartingSide::Both => (
                Some(side(above, self.angle, "above")?),
                Some(side(below, a2, "below")?),
            ),
        };
        Ok(Some(TwoSided {
            plane,
            above,
            below,
        }))
    }
}

/// The plane a parting sketch's lines give: each line swept along the
/// sketch normal; all must lie on one plane square to `pull`.
fn sketch_parting_plane(
    lines: &[(Point3, Point3, Vector3)],
    pull: Vector3,
) -> Result<Plane, String> {
    let mut h: Option<Real> = None;
    for (a, b, n) in lines {
        let m = (b - a).cross(n);
        let ok = m
            .try_normalize(1e-12)
            .is_some_and(|m| m.dot(&pull).abs() > 1.0 - 1e-9);
        if !ok {
            return Err(
                "the parting line is not at one height along the pull direction (not supported yet)"
                    .into(),
            );
        }
        for p in [a, b] {
            let hp = p.coords.dot(&pull);
            match h {
                Some(x) if (x - hp).abs() > 1e-6 => {
                    return Err(
                        "the parting line is not at one height along the pull direction (not supported yet)"
                            .into(),
                    );
                }
                _ => h = Some(hp),
            }
        }
    }
    let h = h.ok_or("the parting sketch has no lines")?;
    Plane::from_normal(Point3::from(pull * h), pull).map_err(|e| e.to_string())
}

/// The world lines of a sketch (ends and the sketch normal), construction
/// lines left out: the parting line of a Parting Line draft.
pub fn sketch_lines(sk: &crate::sketch::Sketch) -> Vec<(Point3, Point3, Vector3)> {
    let n = sk.plane.normal.into_inner();
    sk.entities()
        .filter(|(id, _)| sk.style(*id) == crate::sketch::Style::Normal)
        .filter_map(|(_, e)| match e {
            crate::sketch::Entity::Line { start, end } => {
                let a = sk.point(*start)?.position;
                let b = sk.point(*end)?.position;
                Some((sk.to_world(&a), sk.to_world(&b), n))
            }
            _ => None,
        })
        .collect()
}

/// The model edge `r` names on `body`: the edge shared by faces of its two
/// names nearest its hint.
pub fn edge_of(body: &BRep, r: &EdgeRef) -> Option<EdgeId> {
    let hint = Point3::from(r.hint);
    let opts = TessellationOptions::default();
    body.edges()
        .filter(|(e, _)| {
            let names: Vec<Option<&str>> = body
                .edge_faces(*e)
                .iter()
                .map(|f| body.face(*f).and_then(|x| x.name.as_deref()))
                .collect();
            r.faces.iter().all(|n| names.contains(&Some(n.as_str())))
        })
        .filter_map(|(e, _)| {
            let pts = body.sample_edge(e, &opts).ok()?;
            let d = pts
                .iter()
                .map(|p| (p - hint).norm())
                .fold(Real::INFINITY, Real::min);
            Some((d, e))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, e)| e)
}

/// The fixed edge of `f` for a click at `hint` (Fixed Edge): of its edges
/// square to `pull`, the one nearest the click, and the plane square to
/// `pull` through it. `None` when no edge of the face is square to it.
pub fn fixed_edge(body: &BRep, f: FaceId, hint: Point3, pull: Vector3) -> Option<(EdgeId, Plane)> {
    let opts = TessellationOptions::default();
    let size = body
        .vertices()
        .map(|(_, v)| v.point.coords.norm())
        .fold(1.0, Real::max);
    let mut best: Option<(Real, EdgeId, Point3)> = None;
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
        if best.is_none_or(|(bd, ..)| d < bd) {
            best = Some((d, e, *first));
        }
    }
    let (_, e, p) = best?;
    Some((e, Plane::from_normal(p, pull).ok()?))
}
