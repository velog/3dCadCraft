//! **Combine** (Inventor: 3D Model → Modify → Combine,
//! `docs/spec/2.13-combine-split.md`): a base body joined with, cut by or
//! intersected with other bodies of the part (the toolbodies). Bodies are
//! referenced by their persistent names (`Solid::name`, rule 2).
//!
//! The result keeps the base body's name, so the base's appearance and
//! every reference to it carry over. A consumed toolbody (Keep Toolbody
//! off) is gone from the part after the feature; deleting the feature
//! brings it back (regeneration simply does not consume it any more). A kept
//! toolbody stays an independent body, hidden
//! ([`super::PartDocument::hidden_bodies`]).
//!
//! Not patternable on its own: Inventor repeats a Combine only as part of a
//! patterned solid (pattern the solid instead), so it records no
//! `pattern::ToolSet`.

use serde::{Deserialize, Serialize};

use crate::kernel::exact::{self, ExactOp, ExactSolid};
use crate::kernel::{BRep, BoolOp, SolidId};

use super::boolean::Operation;
use super::exact::Fail;

/// The Combine feature.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CombineFeature {
    /// Immutable identity (`Combine<N>`).
    pub id: String,
    /// Display name.
    pub name: String,
    /// The base body (persistent body name).
    #[serde(default)]
    pub base: String,
    /// The toolbodies (persistent body names).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
    /// Join, Cut or Intersect (New Solid is refused).
    pub operation: Operation,
    /// Keep the toolbodies as independent (hidden) bodies.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub keep_tools: bool,
}

impl CombineFeature {
    /// An empty Combine (Join, tools consumed), as Inventor opens it.
    pub fn new() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            base: String::new(),
            tools: Vec::new(),
            operation: Operation::Join,
            keep_tools: false,
        }
    }

    /// The toolbodies this feature consumes (none when they are kept).
    pub fn consumed(&self) -> &[String] {
        if self.keep_tools { &[] } else { &self.tools }
    }

    /// Checks the inputs before any geometry work.
    pub fn validate(&self) -> Result<(), String> {
        if self.base.is_empty() {
            return Err("select the base body".into());
        }
        if self.tools.is_empty() {
            return Err("select at least one toolbody".into());
        }
        if self.tools.contains(&self.base) {
            return Err("the base body cannot also be a toolbody".into());
        }
        if self.operation == Operation::NewSolid {
            return Err("the operation must be Join, Cut or Intersect".into());
        }
        Ok(())
    }

    /// Clicking body `name`: the base first, then toggles a toolbody.
    /// Returns whether the selection changed.
    pub fn pick(&mut self, name: &str, base_active: bool) -> bool {
        if base_active {
            if self.base == name {
                return false;
            }
            self.tools.retain(|t| t != name);
            self.base = name.to_owned();
            return true;
        }
        if self.base == name {
            return false;
        }
        if let Some(k) = self.tools.iter().position(|t| t == name) {
            self.tools.remove(k);
        } else {
            self.tools.push(name.to_owned());
        }
        true
    }
}

impl Default for CombineFeature {
    fn default() -> Self {
        Self::new()
    }
}

fn find(cur: &[ExactSolid], name: &str) -> Option<usize> {
    cur.iter().position(|s| s.name.as_deref() == Some(name))
}

/// The exact build: the base combined with each tool in turn; consumed
/// tools leave the list. User errors are the feature's health; a kernel
/// failure is `Fail::Kernel` (the part is rebuilt faceted).
pub(super) fn build_exact(c: &CombineFeature, cur: &[ExactSolid]) -> Result<Vec<ExactSolid>, Fail> {
    c.validate().map_err(Fail::User)?;
    let mut out = cur.to_vec();
    let b = find(&out, &c.base)
        .ok_or_else(|| Fail::User(format!("the base body {} no longer exists", c.base)))?;
    let mut base = out[b].clone();
    for t in &c.tools {
        let i = find(&out, t)
            .ok_or_else(|| Fail::User(format!("the toolbody {t} no longer exists")))?;
        let tool = &out[i];
        base = match c.operation {
            Operation::Join => exact::boolean(&base, tool, ExactOp::Union)
                .map_err(|e| Fail::Kernel(format!("Join failed: {e}")))?
                .ok_or_else(|| Fail::Kernel("Join failed: the result is empty".into()))?,
            Operation::Cut => {
                if !exact::meets(&base, tool) {
                    return Err(Fail::User(format!(
                        "Cut: the toolbody {t} does not intersect the base body"
                    )));
                }
                exact::boolean(&base, tool, ExactOp::Cut)
                    .map_err(|e| Fail::Kernel(format!("Cut failed: {e}")))?
                    .ok_or_else(|| Fail::User("Cut: nothing is left of the base body".into()))?
            }
            Operation::Intersect => {
                let r = if exact::meets(&base, tool) {
                    exact::boolean(&base, tool, ExactOp::Intersect)
                        .map_err(|e| Fail::Kernel(format!("Intersect failed: {e}")))?
                } else {
                    None
                };
                r.ok_or_else(|| {
                    Fail::User(format!(
                        "Intersect: the base body and {t} have no volume in common"
                    ))
                })?
            }
            Operation::NewSolid => {
                return Err(Fail::User(
                    "New Solid is not an operation of Combine".into(),
                ));
            }
        };
        // The result is the base (its name, so its appearance).
        base.name = Some(c.base.clone());
    }
    out[b] = base;
    out.retain(|s| s.name.as_ref().is_none_or(|n| !c.consumed().contains(n)));
    Ok(out)
}

fn solid_named(body: &BRep, name: &str) -> Option<SolidId> {
    body.solids()
        .find(|(_, s)| !s.open && s.name.as_deref() == Some(name))
        .map(|(id, _)| id)
}

/// The faceted build (when an earlier feature made the part faceted): the
/// same steps with the BSP Booleans. `body` is changed only on success.
pub(super) fn build_faceted(c: &CombineFeature, body: &mut BRep) -> Result<(), String> {
    c.validate()?;
    let mut trial = body.clone();
    let tools = body.clone();
    let kop = match c.operation {
        Operation::Join => BoolOp::Union,
        Operation::Cut => BoolOp::Subtract,
        Operation::Intersect => BoolOp::Intersect,
        Operation::NewSolid => return Err("New Solid is not an operation of Combine".into()),
    };
    if solid_named(&trial, &c.base).is_none() {
        return Err(format!("the base body {} no longer exists", c.base));
    }
    for t in &c.tools {
        let tid =
            solid_named(&tools, t).ok_or_else(|| format!("the toolbody {t} no longer exists"))?;
        let bid = solid_named(&trial, &c.base)
            .ok_or_else(|| format!("{}: nothing is left of the base body", c.operation.label()))?;
        let made = trial.boolean(bid, &tools, tid, kop).map_err(|e| {
            let e = e.to_string();
            match c.operation {
                Operation::Cut if e.contains("does not intersect") => {
                    format!("Cut: the toolbody {t} does not intersect the base body")
                }
                Operation::Cut if e.contains("empty") => {
                    "Cut: nothing is left of the base body".to_owned()
                }
                Operation::Intersect => {
                    format!("Intersect: the base body and {t} have no volume in common")
                }
                _ => format!("{} failed: {e}", c.operation.label()),
            }
        })?;
        for id in made {
            trial.name_solid(id, &c.base);
        }
    }
    for t in c.consumed() {
        if let Some(id) = solid_named(&trial, t) {
            trial.remove_solid(id);
        }
    }
    *body = trial;
    Ok(())
}
