//! Derive Part (spec 3.10 §3.5, T5): another part's bodies brought into this
//! one as a linked base feature.
//!
//! A derived part is an [`ImportOp`] whose STEP text is **generated** from
//! the source part's exact bodies (filtered, joined, scaled, mirrored as the
//! [`DeriveLink`] says) instead of read from a file. The part keeps that text,
//! so it regenerates from its own data (rule 8: the source is never looked up
//! while it builds, and the part opened alone keeps its last state); Update
//! generates the text again from the source and goes through
//! [`ImportOp::updated`], so faces keep their names and later features their
//! references. Old builds read such a part as a plain Import with the same
//! geometry. Break Link is dropping the link: the text stays.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::import::ImportOp;
use super::{BodyOp, BodyOpFeature, Feature, PartDocument, PartError};
use crate::document::DocRef;
use crate::kernel::{Real, Transform3};

/// What a derived feature reads: the source's parts and where they stand
/// in it (one part at the identity, or every part of an assembly placed),
/// and their signature.
pub struct DeriveSource<'a> {
    /// The parts, each with its placement in the source.
    pub parts: Vec<(&'a PartDocument, Transform3)>,
    /// What changes when any of them changes or moves.
    pub signature: u64,
}

impl<'a> DeriveSource<'a> {
    /// A part as the source.
    pub fn part(p: &'a PartDocument) -> Self {
        Self::of(vec![(p, Transform3::identity())])
    }

    /// Parts placed in an assembly as the source (Derive Assembly).
    pub fn of(parts: Vec<(&'a PartDocument, Transform3)>) -> Self {
        let signature = parts_signature(&parts);
        Self { parts, signature }
    }
}

/// [`signature`] of every part and its placement.
fn parts_signature(parts: &[(&PartDocument, Transform3)]) -> u64 {
    use std::hash::{Hash, Hasher};
    if let [(p, t)] = parts
        && *t == Transform3::identity()
    {
        return signature(p);
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for (p, t) in parts {
        signature(p).hash(&mut h);
        for v in t.to_homogeneous().iter() {
            v.to_bits().hash(&mut h);
        }
    }
    h.finish()
}
use crate::kernel::exact::{self, ExactOp, ExactSolid};

/// How the derived bodies are kept (Inventor's Derive Style).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeriveStyle {
    /// One solid: the bodies joined, coplanar faces merged.
    SingleMerged,
    /// One solid: the bodies joined, faces kept.
    Single,
    /// Each solid body a body of this part (Inventor's default for parts).
    #[default]
    Each,
    /// The bodies as surface bodies.
    Surfaces,
}

/// A mirror plane of the derived geometry (an origin plane of the source).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MirrorPlane {
    /// Mirrored about XY (z → −z).
    Xy,
    /// About XZ (y → −y).
    Xz,
    /// About YZ (x → −x).
    Yz,
}

/// What a derived part follows and how.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeriveLink {
    /// The source part: a document of this file or of a linked file.
    pub source: DocRef,
    /// Its name when derived (shown when it cannot be found).
    pub name: String,
    /// Derive Style.
    #[serde(default)]
    pub style: DeriveStyle,
    /// Include solid bodies.
    #[serde(default = "yes")]
    pub solids: bool,
    /// Include surface bodies.
    #[serde(default)]
    pub surfaces: bool,
    /// Bodies of the source left out (by persistent name).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,
    /// Scale Factor (1 = as is), about the source's origin.
    #[serde(default = "one")]
    pub scale: Real,
    /// Mirror Part: the plane, when mirrored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mirror: Option<MirrorPlane>,
    /// Include Parameters: the source part's user parameters become linked
    /// parameters of this part (spec 3.10 §3.9).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub parameters: bool,
    /// The text was generated from a source whose bodies had this signature
    /// (Update needed when the source's differs).
    #[serde(default)]
    pub signature: u64,
}

fn yes() -> bool {
    true
}

fn one() -> Real {
    1.0
}

impl DeriveLink {
    /// Derive `source` (named `name`) as is: each solid a body.
    pub fn new(source: DocRef, name: impl Into<String>) -> Self {
        Self {
            source,
            name: name.into(),
            style: DeriveStyle::Each,
            solids: true,
            surfaces: false,
            exclude: Vec::new(),
            scale: 1.0,
            mirror: None,
            parameters: false,
            signature: 0,
        }
    }

    /// The source's bodies as this link takes them: each part's bodies
    /// placed where the part stands, filtered, joined (single styles),
    /// scaled and mirrored; and the source's signature.
    pub fn bodies(&self, source: &DeriveSource) -> Result<(Vec<ExactSolid>, u64), String> {
        if !(self.scale.is_finite() && self.scale > 0.0) {
            return Err("the scale factor must be greater than 0".into());
        }
        let mut kept: Vec<ExactSolid> = Vec::new();
        for (part, at) in &source.parts {
            if !part.exact {
                return Err(format!(
                    "{} is not built by the exact kernel (it cannot be derived)",
                    self.name
                ));
            }
            for s in &part.exact_bodies.0 {
                let name = s.name.as_deref().unwrap_or("");
                let open = s.is_open();
                if self.exclude.iter().any(|x| x == name)
                    || !(if open { self.surfaces } else { self.solids })
                    || part.hidden_bodies.iter().any(|h| h == name)
                {
                    continue;
                }
                kept.push(if *at == Transform3::identity() {
                    s.clone()
                } else {
                    exact::similar(s, &at.to_homogeneous())?
                });
            }
        }
        if kept.is_empty() {
            return Err(format!("{} has no body the filters keep", self.name));
        }
        if matches!(self.style, DeriveStyle::Single | DeriveStyle::SingleMerged) && kept.len() > 1 {
            let (solids, open): (Vec<_>, Vec<_>) = kept.into_iter().partition(|s| !s.is_open());
            let mut joined: Option<ExactSolid> = None;
            for s in solids {
                joined = Some(match joined {
                    None => s,
                    Some(j) => exact::boolean(&j, &s, ExactOp::Union)?
                        .ok_or("the bodies could not be joined into one")?,
                });
            }
            kept = joined.into_iter().chain(open).collect();
        }
        let mut m = nalgebra::Matrix4::<Real>::identity();
        if (self.scale - 1.0).abs() > 1e-12 {
            m = nalgebra::Matrix4::new_scaling(self.scale) * m;
        }
        if let Some(p) = self.mirror {
            let mut r = nalgebra::Matrix4::<Real>::identity();
            let k = match p {
                MirrorPlane::Xy => 2,
                MirrorPlane::Xz => 1,
                MirrorPlane::Yz => 0,
            };
            r[(k, k)] = -1.0;
            m = r * m;
        }
        if m != nalgebra::Matrix4::identity() {
            kept = kept
                .iter()
                .map(|s| exact::similar(s, &m))
                .collect::<Result<_, _>>()?;
        }
        Ok((kept, source.signature))
    }

    /// The STEP text of [`Self::bodies`] and the source's signature.
    pub fn text(&self, source: &DeriveSource) -> Result<(Arc<str>, u64), String> {
        let (bodies, sig) = self.bodies(source)?;
        let text = exact::step_text(&bodies, &self.name)?;
        Ok((text.into(), sig))
    }
}

/// A signature of `part`'s bodies as derived (its features and sketches as
/// saved: what changes the bodies). Update is needed when it differs from
/// the one a link was made with.
pub fn signature(part: &PartDocument) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_vec(&part.features)
        .unwrap_or_default()
        .hash(&mut h);
    serde_json::to_vec(&part.sketches)
        .unwrap_or_default()
        .hash(&mut h);
    part.hidden_bodies.hash(&mut h);
    h.finish()
}

impl PartDocument {
    /// **Derive**: the bodies of `source` (as `link` takes them) become this
    /// part's base feature (an Import of the text they make, linked back).
    /// Returns the feature's id. Nothing changes on failure.
    pub fn add_derive(
        &mut self,
        source: &PartDocument,
        link: DeriveLink,
    ) -> Result<String, PartError> {
        self.add_derive_from(&DeriveSource::part(source), link)
    }

    /// [`Self::add_derive`] of any source (an assembly's parts placed).
    pub fn add_derive_from(
        &mut self,
        source: &DeriveSource,
        mut link: DeriveLink,
    ) -> Result<String, PartError> {
        let (text, sig) = link.text(source).map_err(PartError::Feature)?;
        link.signature = sig;
        let name = link.name.clone();
        // Include Parameters: the source part's user parameters, linked.
        let params: Vec<(String, Real, crate::part::expr::Unit)> = match source.parts.as_slice() {
            [(p, _)] if link.parameters => p
                .user_params
                .iter()
                .map(|u| (u.name.clone(), u.value, u.unit))
                .collect(),
            _ => Vec::new(),
        };
        let before = self.clone();
        for (n, v, u) in params {
            let l = crate::part::params::ParamLink::Document {
                source: link.source,
                name: n.clone(),
            };
            if let Err(e) = self.add_linked_parameter(&n, v, u, l) {
                *self = before;
                return Err(e);
            }
        }
        let mut op = ImportOp::new(link.name.clone(), text);
        op.surfaces = true;
        op.derive = Some(link);
        let id = match self.add_body_op(BodyOpFeature::new(BodyOp::Import(op))) {
            Ok(id) => id,
            Err(e) => {
                *self = before;
                return Err(e);
            }
        };
        // Shown under the source's name (Inventor lists the derived file).
        if let Some(Feature::BodyOp(b)) = self.features.iter_mut().find(|f| f.id() == id) {
            b.name = name;
        }
        Ok(id)
    }

    /// The derived features of this part: (index, link).
    pub fn derived(&self) -> Vec<(usize, &DeriveLink)> {
        self.features
            .iter()
            .enumerate()
            .filter_map(|(k, f)| match f {
                Feature::BodyOp(b) => match &b.op {
                    BodyOp::Import(o) => o.derive.as_ref().map(|d| (k, d)),
                    _ => None,
                },
                _ => None,
            })
            .collect()
    }

    /// **Update** of derived feature `k` from its source `source` (now):
    /// the text is generated again; faces matched by geometry keep their
    /// names. `Ok(false)` when the source did not change. Nothing changes on
    /// failure.
    pub fn update_derive(&mut self, k: usize, source: &PartDocument) -> Result<bool, PartError> {
        self.update_derive_from(k, &DeriveSource::part(source))
    }

    /// [`Self::update_derive`] from any source.
    pub fn update_derive_from(
        &mut self,
        k: usize,
        source: &DeriveSource,
    ) -> Result<bool, PartError> {
        let fail = |m: String| PartError::Feature(format!("Derive: {m}"));
        let Some(Feature::BodyOp(b)) = self.features.get(k) else {
            return Err(fail("no such feature".into()));
        };
        let BodyOp::Import(op) = &b.op else {
            return Err(fail("not a derived feature".into()));
        };
        let Some(link) = op.derive.clone() else {
            return Err(fail("not a derived feature".into()));
        };
        if source.signature == link.signature {
            return Ok(false);
        }
        let (text, sig) = link.text(source).map_err(fail)?;
        let (mut next, _) = op.updated(&b.id, text).map_err(fail)?;
        next.derive = Some(DeriveLink {
            signature: sig,
            ..link
        });
        let mut def = b.clone();
        def.op = BodyOp::Import(next);
        let before = self.features[k].clone();
        self.features[k] = Feature::BodyOp(def);
        if let Err(e) = self.regenerate() {
            self.features[k] = before;
            let _ = self.regenerate();
            return Err(e);
        }
        Ok(true)
    }

    /// **Edit Feature** of derived feature `k`: its options become `link`'s
    /// (style, filters, scale, mirror, even another source) and the text is
    /// written again from `source`; faces matched by geometry keep their
    /// names. Nothing changes on failure.
    pub fn edit_derive(
        &mut self,
        k: usize,
        source: &DeriveSource,
        mut link: DeriveLink,
    ) -> Result<(), PartError> {
        let before = self.features.get(k).cloned();
        let Some(Feature::BodyOp(b)) = self.features.get_mut(k) else {
            return Err(PartError::Feature("Derive: no such feature".into()));
        };
        let BodyOp::Import(op) = &mut b.op else {
            return Err(PartError::Feature("Derive: not a derived feature".into()));
        };
        // Signature 0: the update below always writes the text again.
        link.signature = 0;
        let name = link.name.clone();
        op.derive = Some(link);
        b.name = name;
        match self.update_derive_from(k, source) {
            Ok(_) => Ok(()),
            Err(e) => {
                if let Some(f) = before {
                    self.features[k] = f;
                    let _ = self.regenerate();
                }
                Err(e)
            }
        }
    }

    /// **Break Link** of derived feature `k`: it stays as it is, a plain
    /// Import of the text it holds.
    pub fn break_derive(&mut self, k: usize) -> bool {
        match self.features.get_mut(k) {
            Some(Feature::BodyOp(b)) => match &mut b.op {
                BodyOp::Import(o) => o.derive.take().is_some(),
                _ => false,
            },
            _ => false,
        }
    }
}
