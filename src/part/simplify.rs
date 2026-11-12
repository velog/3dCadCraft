//! Simplify (spec 2.18 for a part, 3.11 for an assembly): a lighter stand-in for a model —
//! features taken out by kind and size, bodies or parts left out, internal ones removed, voids
//! filled, envelopes in place of the bodies.
//!
//! In a part, Simplify is a feature at the end of the history ([`SimplifyOp`] =
//! `BodyOp::Simplify`, format revision 74). Two halves:
//!
//! - **features** (holes, fillets, chamfers, pockets, embosses) are recognised from the
//!   history: the features they are, and their sizes, are known exactly. The ones the rules
//!   remove are treated as suppressed while the Simplify feature is active
//!   ([`PartDocument::simplified_away`], gathered with the Suppress if conditions), together
//!   with what depends on them;
//! - **bodies** (exclusions, internal bodies, voids, envelopes) are the feature's own step at its
//!   place in the history ([`simplify_bodies`]).
//!
//! As a variant (Inventor: a model state) it is a variant whose Simplify feature is active, the
//! others suppress it ([`PartDocument::add_simplified_variant`]): the master's changes reach it
//! and Edit Feature changes how much it simplifies. As another part it is a derived part whose
//! link carries the [`SimplifySpec`] (`part::derive`), assemblies included.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::{BodyOp, BodyOpFeature, Feature, Operation, PartDocument, PartError};
use crate::kernel::Real;
use crate::kernel::exact::simplify::{
    EnvelopeShape, envelope, fill_voids, mesh_points, mesh_triangles, visible_from_outside,
};
use crate::kernel::exact::{self, ExactSolid};

/// How a kind of feature is removed (Inventor's None / All / Range / Preserve).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RemoveMode {
    /// Kept.
    #[default]
    None,
    /// Every one removed.
    All,
    /// Those of a size up to the maximum removed.
    Range,
    /// Every one removed but those chosen to stay ([`SimplifySpec::preserve`]).
    Preserve,
}

/// One kind of feature's rule.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Removal {
    /// How.
    #[serde(default)]
    pub mode: RemoveMode,
    /// The largest size removed with [`RemoveMode::Range`] (mm).
    #[serde(default)]
    pub max: Real,
}

impl Removal {
    /// Removes features up to `max` mm.
    pub fn range(max: Real) -> Self {
        Self {
            mode: RemoveMode::Range,
            max,
        }
    }

    /// Removes every one.
    pub fn all() -> Self {
        Self {
            mode: RemoveMode::All,
            max: 0.0,
        }
    }

    fn takes(&self, size: Real, kept: bool) -> bool {
        match self.mode {
            RemoveMode::None => false,
            RemoveMode::All => !kept,
            RemoveMode::Preserve => !kept,
            RemoveMode::Range => !kept && size <= self.max * (1.0 + 1e-9) + 1e-9,
        }
    }

    fn is_none(&self) -> bool {
        self.mode == RemoveMode::None
    }
}

/// What envelopes replace (Inventor's Replace with Envelopes).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Envelope {
    /// No envelope.
    #[default]
    None,
    /// One envelope round everything (a part's Whole Part, an assembly's All in One Envelope).
    All,
    /// One per body of the part.
    EachBody,
    /// One per chosen body ([`SimplifySpec::envelope_bodies`]).
    Selected,
    /// One per first-level component of an assembly.
    EachTop,
    /// One per part of an assembly (every occurrence).
    EachPart,
}

/// The envelope's shape (Inventor's Type).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum EnvelopeType {
    /// Orthogonal Bounding Box.
    #[default]
    Box,
    /// Oriented Minimum Bounding Box.
    OrientedBox,
    /// Oriented Minimum Bounding Cylinder.
    Cylinder,
}

impl EnvelopeType {
    fn shape(self) -> EnvelopeShape {
        match self {
            EnvelopeType::Box => EnvelopeShape::Box,
            EnvelopeType::OrientedBox => EnvelopeShape::OrientedBox,
            EnvelopeType::Cylinder => EnvelopeShape::Cylinder,
        }
    }
}

/// The kinds of features Simplify recognises.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FeatureKind {
    /// Hole features, and cuts whose profiles are only circles. Size: the largest diameter.
    Hole,
    /// Fillets. Size: the largest radius.
    Fillet,
    /// Chamfers. Size: the largest distance.
    Chamfer,
    /// Other cuts (extruded, revolved, swept, engraved). Size: the profile's diagonal.
    Pocket,
    /// Additions on the body (joined extrusions, revolutions, sweeps, embosses). Size: the
    /// profile's diagonal.
    Emboss,
}

/// Inventor's built-in presets (ours fill the fields with sizes relative to the model).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    /// No Simplification.
    None,
    /// Remove the least detail: small holes, fillets and chamfers, small bodies.
    Least,
    /// Remove moderate detail: every hole, fillet, chamfer, small pockets and embosses, bodies
    /// up to a tenth of the model.
    Moderate,
    /// Remove the most detail: envelopes in place of the bodies (or first-level components).
    Most,
}

/// Every option of Simplify.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SimplifySpec {
    /// Replace with Envelopes.
    #[serde(default)]
    pub envelope: Envelope,
    /// The envelopes' shape.
    #[serde(default)]
    pub envelope_type: EnvelopeType,
    /// The bodies [`Envelope::Selected`] replaces (by persistent name).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub envelope_bodies: Vec<String>,
    /// Bodies left out by name (a part), or occurrences by id path `3/7` (an assembly: a path
    /// leaves out everything below it).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,
    /// Exclude by size: bodies (parts) whose bounding box diagonal is at most this (mm).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude_size: Option<Real>,
    /// Holes.
    #[serde(default, skip_serializing_if = "Removal::is_none")]
    pub holes: Removal,
    /// Fillets.
    #[serde(default, skip_serializing_if = "Removal::is_none")]
    pub fillets: Removal,
    /// Chamfers.
    #[serde(default, skip_serializing_if = "Removal::is_none")]
    pub chamfers: Removal,
    /// Pockets.
    #[serde(default, skip_serializing_if = "Removal::is_none")]
    pub pockets: Removal,
    /// Embosses.
    #[serde(default, skip_serializing_if = "Removal::is_none")]
    pub embosses: Removal,
    /// Features that stay whatever the rules say (by id).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preserve: Vec<String>,
    /// Fill internal voids.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fill_voids: bool,
    /// Remove internal bodies (parts): those no ray from the 14 view directions reaches.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub remove_internal: bool,
}

impl SimplifySpec {
    /// The rule of a kind of feature.
    pub fn rule(&self, kind: FeatureKind) -> Removal {
        match kind {
            FeatureKind::Hole => self.holes,
            FeatureKind::Fillet => self.fillets,
            FeatureKind::Chamfer => self.chamfers,
            FeatureKind::Pocket => self.pockets,
            FeatureKind::Emboss => self.embosses,
        }
    }

    /// Whether any feature rule removes something.
    pub fn removes_features(&self) -> bool {
        [
            self.holes,
            self.fillets,
            self.chamfers,
            self.pockets,
            self.embosses,
        ]
        .iter()
        .any(|r| !r.is_none())
    }

    /// Only the feature rules (what each part of an assembly does by itself).
    pub fn features_only(&self) -> Self {
        Self {
            holes: self.holes,
            fillets: self.fillets,
            chamfers: self.chamfers,
            pockets: self.pockets,
            embosses: self.embosses,
            preserve: self.preserve.clone(),
            ..Self::default()
        }
    }

    /// A preset's values for a model of size `size` (its bounding box diagonal, mm);
    /// `assembly`: an assembly's envelopes (first-level components) instead of a part's.
    pub fn preset(p: Preset, size: Real, assembly: bool) -> Self {
        let size = size.max(1e-3);
        match p {
            Preset::None => Self::default(),
            Preset::Least => Self {
                holes: Removal::range(0.02 * size),
                fillets: Removal::range(0.02 * size),
                chamfers: Removal::range(0.02 * size),
                exclude_size: Some(0.02 * size),
                remove_internal: true,
                ..Self::default()
            },
            Preset::Moderate => Self {
                holes: Removal::all(),
                fillets: Removal::all(),
                chamfers: Removal::all(),
                pockets: Removal::range(0.1 * size),
                embosses: Removal::range(0.1 * size),
                exclude_size: Some(0.1 * size),
                remove_internal: true,
                fill_voids: true,
                ..Self::default()
            },
            Preset::Most => Self {
                envelope: if assembly {
                    Envelope::EachTop
                } else {
                    Envelope::EachBody
                },
                remove_internal: true,
                ..Self::default()
            },
        }
    }

    /// Refuses values that cannot work.
    pub fn validate(&self) -> Result<(), String> {
        for (what, r) in [
            ("Holes", self.holes),
            ("Fillets", self.fillets),
            ("Chamfers", self.chamfers),
            ("Pockets", self.pockets),
            ("Embosses", self.embosses),
        ] {
            if r.mode == RemoveMode::Range && !(r.max.is_finite() && r.max > 0.0) {
                return Err(format!("{what}: the maximum size must be greater than 0"));
            }
        }
        if let Some(d) = self.exclude_size
            && !(d.is_finite() && d >= 0.0)
        {
            return Err("Max. Diagonal must not be negative".into());
        }
        if self.envelope == Envelope::Selected && self.envelope_bodies.is_empty() {
            return Err("choose the bodies the envelopes replace".into());
        }
        Ok(())
    }

    /// A short description (the browser's tooltip).
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        for (what, r) in [
            ("holes", self.holes),
            ("fillets", self.fillets),
            ("chamfers", self.chamfers),
            ("pockets", self.pockets),
            ("embosses", self.embosses),
        ] {
            match r.mode {
                RemoveMode::None => {}
                RemoveMode::All | RemoveMode::Preserve => parts.push(format!("all {what}")),
                RemoveMode::Range => parts.push(format!("{what} up to {} mm", r.max)),
            }
        }
        if !self.exclude.is_empty() {
            parts.push(format!("{} left out", self.exclude.len()));
        }
        if let Some(d) = self.exclude_size {
            parts.push(format!("bodies up to {d} mm left out"));
        }
        if self.remove_internal {
            parts.push("internal bodies removed".into());
        }
        if self.fill_voids {
            parts.push("voids filled".into());
        }
        if self.envelope != Envelope::None {
            parts.push(format!(
                "envelopes ({:?}, {:?})",
                self.envelope, self.envelope_type
            ));
        }
        if parts.is_empty() {
            "Simplify: nothing removed".into()
        } else {
            format!("Simplify: {}", parts.join(", "))
        }
    }
}

/// The Simplify feature (`BodyOp::Simplify`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SimplifyOp {
    /// Its options.
    #[serde(flatten)]
    pub spec: SimplifySpec,
}

/// The diagonal of a sketch's own curves (construction left out), mm; `None` for no curve.
/// `circles`: whether every curve is a whole circle (a hole drawn as a cut), with the largest
/// diameter.
fn sketch_size(s: &crate::sketch::Sketch) -> Option<(Real, Option<Real>)> {
    use crate::sketch::{Entity, Style};
    let mut lo = [Real::INFINITY; 2];
    let mut hi = [Real::NEG_INFINITY; 2];
    let mut grow = |x: Real, y: Real, r: Real| {
        lo[0] = lo[0].min(x - r);
        lo[1] = lo[1].min(y - r);
        hi[0] = hi[0].max(x + r);
        hi[1] = hi[1].max(y + r);
    };
    let at = |p| s.point(p).map(|q| q.position);
    let mut only_circles = true;
    let mut largest: Real = 0.0;
    let mut any = false;
    for (id, e) in s.entities() {
        if s.style(id) != Style::Normal {
            continue;
        }
        any = true;
        match e {
            Entity::Line { start, end } => {
                only_circles = false;
                for p in [start, end].into_iter().filter_map(|p| at(*p)) {
                    grow(p.x, p.y, 0.0);
                }
            }
            Entity::Circle { center, radius } => {
                largest = largest.max(2.0 * radius);
                if let Some(c) = at(*center) {
                    grow(c.x, c.y, *radius);
                }
            }
            Entity::Arc { center, start, .. } => {
                only_circles = false;
                if let (Some(c), Some(p)) = (at(*center), at(*start)) {
                    grow(c.x, c.y, (p - c).norm());
                }
            }
            Entity::Spline { points } => {
                only_circles = false;
                for p in points.iter().filter_map(|p| at(*p)) {
                    grow(p.x, p.y, 0.0);
                }
            }
        }
    }
    if !any || !lo[0].is_finite() {
        return None;
    }
    let diag = ((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2)).sqrt();
    Some((diag, only_circles.then_some(largest)))
}

impl PartDocument {
    fn sketch_by_id(&self, id: &str) -> Option<&crate::sketch::Sketch> {
        self.sketches.iter().find(|s| s.id == id).map(|s| &s.sketch)
    }

    /// What Simplify takes feature `k` for: its kind and size (mm). `None`: a feature Simplify
    /// never removes (base features, work features, patterns — a pattern goes with its
    /// source).
    pub fn simplify_kind(&self, k: usize) -> Option<(FeatureKind, Real)> {
        let cut_or_join = |sketch: &str, op: Operation| -> Option<(FeatureKind, Real)> {
            let (size, circles) = self
                .sketch_by_id(sketch)
                .and_then(sketch_size)
                .unwrap_or((Real::INFINITY, None));
            match op {
                Operation::Cut => Some(match circles {
                    Some(d) => (FeatureKind::Hole, d),
                    None => (FeatureKind::Pocket, size),
                }),
                Operation::Join => Some((FeatureKind::Emboss, size)),
                _ => None,
            }
        };
        let only_size = |sketch: &str| {
            self.sketch_by_id(sketch)
                .and_then(sketch_size)
                .map_or(Real::INFINITY, |s| s.0)
        };
        match self.features.get(k)? {
            Feature::Hole(h) => {
                let s = &h.spec;
                let seat = match s.seat {
                    super::hole::SeatType::None => 0.0,
                    super::hole::SeatType::Countersink => s.csink_diameter,
                    _ => s.seat_diameter,
                };
                Some((FeatureKind::Hole, s.diameter.max(seat)))
            }
            Feature::Blend(b) => match b.kind {
                super::blend::BlendKind::Fillet { radius } => Some((
                    FeatureKind::Fillet,
                    b.sets.iter().map(|s| s.radius).fold(radius, Real::max),
                )),
                super::blend::BlendKind::Chamfer {
                    distance,
                    distance2,
                    ..
                } => Some((FeatureKind::Chamfer, distance.max(distance2))),
            },
            Feature::Extrude(e) => cut_or_join(&e.sketch, e.operation),
            Feature::Revolve(r) => match r.operation {
                Operation::Cut => Some((FeatureKind::Pocket, only_size(&r.sketch))),
                Operation::Join => Some((FeatureKind::Emboss, only_size(&r.sketch))),
                _ => None,
            },
            Feature::Sweep(s) => match s.operation {
                Operation::Cut => Some((FeatureKind::Pocket, only_size(&s.sketch))),
                Operation::Join => Some((FeatureKind::Emboss, only_size(&s.sketch))),
                _ => None,
            },
            Feature::BodyOp(b) => match &b.op {
                BodyOp::Emboss(o) => Some((
                    match o.style {
                        super::emboss::EmbossKind::EngraveFromFace => FeatureKind::Pocket,
                        _ => FeatureKind::Emboss,
                    },
                    only_size(&o.sketch),
                )),
                _ => None,
            },
            _ => None,
        }
    }

    /// The active Simplify feature (not suppressed, before the End of Part): its index and
    /// options.
    pub fn active_simplify(&self) -> Option<(usize, &SimplifySpec)> {
        let limit = self.build_limit();
        self.features
            .iter()
            .enumerate()
            .take(limit)
            .filter(|(_, f)| {
                !self.suppressed.contains(f.id()) && !self.cond_suppressed.contains(f.id())
            })
            .find_map(|(k, f)| match f {
                Feature::BodyOp(BodyOpFeature {
                    op: BodyOp::Simplify(o),
                    ..
                }) => Some((k, &o.spec)),
                _ => None,
            })
    }

    /// The features the active Simplify feature takes out (by id), with what depends on them.
    pub fn simplified_away(&self) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        let Some((at, spec)) = self.active_simplify() else {
            return out;
        };
        for k in 0..at {
            let f = &self.features[k];
            if self.suppressed.contains(f.id()) {
                continue;
            }
            let Some((kind, size)) = self.simplify_kind(k) else {
                continue;
            };
            let kept = spec.preserve.iter().any(|p| p == f.id());
            if spec.rule(kind).takes(size, kept) {
                out.insert(f.id().to_owned());
                for j in self.dependents(k) {
                    if j < at {
                        out.insert(self.features[j].id().to_owned());
                    }
                }
            }
        }
        out
    }

    /// The features of the history Simplify can take out, before the end (for the panel's
    /// lists): (id, kind, size).
    pub fn simplify_candidates(&self) -> Vec<(String, FeatureKind, Real)> {
        let end = self
            .active_simplify()
            .map_or(self.features.len(), |(k, _)| k);
        (0..end)
            .filter_map(|k| {
                let (kind, size) = self.simplify_kind(k)?;
                Some((self.features[k].id().to_owned(), kind, size))
            })
            .collect()
    }

    /// **Simplify** as a feature at the end of the history. Returns its id; nothing changes on
    /// failure.
    pub fn add_simplify(&mut self, spec: SimplifySpec) -> Result<String, PartError> {
        spec.validate().map_err(PartError::Feature)?;
        self.add_body_op(BodyOpFeature::new(BodyOp::Simplify(SimplifyOp { spec })))
    }

    /// **Simplify → New Model State**: a new variant (named `name`, else `Simplified<k>`) whose
    /// Simplify feature is active; every other variant suppresses that feature, so they stay as
    /// they were. The new variant is active. Returns (variant id, feature id); nothing changes
    /// on failure.
    pub fn add_simplified_variant(
        &mut self,
        spec: SimplifySpec,
        name: Option<&str>,
    ) -> Result<(u32, String), PartError> {
        spec.validate().map_err(PartError::Feature)?;
        let before = self.clone();
        let result = (|| {
            let v = self.new_variant();
            let name = match name.map(str::trim).filter(|n| !n.is_empty()) {
                Some(n) if self.variants.name_free(n, "") => n.to_owned(),
                Some(n) => return Err(PartError::Feature(format!("a variant is named {n}"))),
                None => super::variants::next_name(
                    "Simplified",
                    self.variants.list.iter().map(|v| v.name.as_str()),
                ),
            };
            if let Some(entry) = self.variants.list.iter_mut().find(|x| x.id == v) {
                entry.name = name;
            }
            let id = self.add_simplify(spec)?;
            self.variants_sync();
            Ok((v, id))
        })();
        if result.is_err() {
            *self = before;
        }
        result
    }

    /// The part as Simplify with `spec` leaves it: a copy with a Simplify feature at its end,
    /// built. A part whose own Simplify is already active gets the new one after it.
    pub fn simplified(&self, spec: &SimplifySpec) -> Result<PartDocument, String> {
        let mut p = self.clone();
        p.eop = None;
        p.add_simplify(spec.clone()).map_err(|e| e.to_string())?;
        if let Some((id, h)) = p
            .feature_health
            .iter()
            .find(|(id, _)| id.starts_with("Simplify"))
        {
            return Err(format!("{id}: {h}"));
        }
        Ok(p)
    }
}

/// The diagonal of a body's box, mm.
fn diagonal(s: &ExactSolid) -> Real {
    exact::bounds(s).map_or(0.0, |(lo, hi)| (hi - lo).norm())
}

/// The bodies' step of the Simplify feature `id` of a part: `solids` and `sheets` (surface
/// bodies) in; the bodies it leaves out. Feature removal happened before (the features are
/// suppressed).
pub fn simplify_bodies(
    spec: &SimplifySpec,
    id: &str,
    solids: &[ExactSolid],
    sheets: &[ExactSolid],
) -> Result<Vec<ExactSolid>, String> {
    let mut kept: Vec<ExactSolid> = solids
        .iter()
        .chain(sheets)
        .filter(|s| {
            let name = s.name.as_deref().unwrap_or("");
            !spec.exclude.iter().any(|x| x == name)
                && spec.exclude_size.is_none_or(|d| diagonal(s) > d)
        })
        .cloned()
        .collect();
    if kept.is_empty() {
        return Err("every body is left out".into());
    }
    if spec.remove_internal && kept.len() > 1 {
        let meshes: Vec<_> = kept.iter().map(mesh_triangles).collect::<Result<_, _>>()?;
        let seen = visible_from_outside(&meshes, 96);
        kept = kept
            .into_iter()
            .zip(seen)
            .filter_map(|(s, v)| v.then_some(s))
            .collect();
    }
    if spec.fill_voids {
        kept = kept
            .into_iter()
            .map(|s| {
                if s.is_open() {
                    Ok(s)
                } else {
                    fill_voids(&s).map(|r| r.0)
                }
            })
            .collect::<Result<_, String>>()?;
    }
    let shape = spec.envelope_type.shape();
    match spec.envelope {
        Envelope::None => Ok(kept),
        Envelope::All | Envelope::EachTop => {
            let mut pts = Vec::new();
            for s in &kept {
                pts.extend(mesh_points(s)?);
            }
            Ok(vec![envelope(&pts, shape, id)?])
        }
        Envelope::EachBody | Envelope::EachPart | Envelope::Selected => kept
            .into_iter()
            .map(|s| {
                let name = s.name.clone().unwrap_or_default();
                if spec.envelope == Envelope::Selected
                    && !spec.envelope_bodies.iter().any(|b| *b == name)
                {
                    return Ok(s);
                }
                let mut e = envelope(&mesh_points(&s)?, shape, &format!("{id}:{name}"))?;
                e.name = s.name.clone();
                Ok(e)
            })
            .collect(),
    }
}

/// An assembly's parts as Simplify takes them: each part's feature rules applied by itself,
/// its bodies placed, then the parts left out (by `spec.exclude` id paths and by size),
/// internal parts removed, voids filled and envelopes made over the assembly (`EachTop` groups
/// by the first-level occurrence). `parts`: (built part, placement, occurrence id path).
pub fn simplify_assembly(
    spec: &SimplifySpec,
    parts: &[(&PartDocument, crate::kernel::Transform3, Vec<u32>)],
    solids: bool,
    surfaces: bool,
) -> Result<Vec<ExactSolid>, String> {
    let per_part = spec.features_only();
    let excluded = |path: &[u32]| {
        let key = path
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join("/");
        spec.exclude
            .iter()
            .any(|x| key == *x || key.starts_with(&format!("{x}/")))
    };
    // (bodies of one part placed, its first-level group)
    let mut groups: Vec<(Vec<ExactSolid>, u32)> = Vec::new();
    for (part, at, path) in parts {
        if excluded(path) {
            continue;
        }
        let built;
        let part: &PartDocument = if per_part.removes_features() {
            built = part.simplified(&per_part)?;
            &built
        } else {
            part
        };
        let mut bodies = Vec::new();
        for s in &part.exact_bodies.0 {
            let name = s.name.as_deref().unwrap_or("");
            if !(if s.is_open() { surfaces } else { solids })
                || part.hidden_bodies.iter().any(|h| h == name)
            {
                continue;
            }
            bodies.push(if *at == crate::kernel::Transform3::identity() {
                s.clone()
            } else {
                exact::similar(s, &at.to_homogeneous())?
            });
        }
        if bodies.is_empty() {
            continue;
        }
        if let Some(d) = spec.exclude_size {
            let mut pts = Vec::new();
            for b in &bodies {
                if let Some((lo, hi)) = exact::bounds(b) {
                    pts.extend([lo, hi]);
                }
            }
            let lo = pts.iter().fold(None::<crate::kernel::Point3>, |a, p| {
                Some(a.map_or(*p, |a| a.inf(p)))
            });
            let hi = pts.iter().fold(None::<crate::kernel::Point3>, |a, p| {
                Some(a.map_or(*p, |a| a.sup(p)))
            });
            if let (Some(lo), Some(hi)) = (lo, hi)
                && (hi - lo).norm() <= d
            {
                continue;
            }
        }
        groups.push((bodies, path.first().copied().unwrap_or(0)));
    }
    if groups.is_empty() {
        return Err("every component is left out".into());
    }
    if spec.remove_internal && groups.len() > 1 {
        let meshes: Vec<Vec<[crate::kernel::Point3; 3]>> = groups
            .iter()
            .map(|(b, _)| -> Result<_, String> {
                let mut t = Vec::new();
                for s in b {
                    t.extend(mesh_triangles(s)?);
                }
                Ok(t)
            })
            .collect::<Result<_, _>>()?;
        let seen = visible_from_outside(&meshes, 96);
        groups = groups
            .into_iter()
            .zip(seen)
            .filter_map(|(g, v)| v.then_some(g))
            .collect();
    }
    if spec.fill_voids {
        for (bodies, _) in &mut groups {
            for b in bodies.iter_mut() {
                if !b.is_open() {
                    *b = fill_voids(b)?.0;
                }
            }
        }
    }
    let shape = spec.envelope_type.shape();
    let points = |bodies: &[ExactSolid]| -> Result<Vec<crate::kernel::Point3>, String> {
        let mut p = Vec::new();
        for b in bodies {
            p.extend(mesh_points(b)?);
        }
        Ok(p)
    };
    match spec.envelope {
        Envelope::None | Envelope::Selected => Ok(groups.into_iter().flat_map(|g| g.0).collect()),
        Envelope::All => {
            let all: Vec<ExactSolid> = groups.into_iter().flat_map(|g| g.0).collect();
            Ok(vec![envelope(&points(&all)?, shape, "Envelope")?])
        }
        Envelope::EachTop => {
            let mut tops: Vec<(u32, Vec<ExactSolid>)> = Vec::new();
            for (b, top) in groups {
                match tops.iter_mut().find(|t| t.0 == top) {
                    Some(t) => t.1.extend(b),
                    None => tops.push((top, b)),
                }
            }
            tops.into_iter()
                .map(|(top, b)| envelope(&points(&b)?, shape, &format!("Envelope{top}")))
                .collect()
        }
        Envelope::EachPart | Envelope::EachBody => groups
            .into_iter()
            .enumerate()
            .map(|(k, (b, _))| envelope(&points(&b)?, shape, &format!("Envelope{}", k + 1)))
            .collect(),
    }
}
