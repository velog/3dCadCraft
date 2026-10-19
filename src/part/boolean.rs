//! What a feature does with the body: Inventor's **Output** (Solid /
//! Surface) and **Operation** (Join / Cut / Intersect / New Solid), shared
//! by Extrude and Revolve (`docs/spec/2.13-booleans.md`).

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::kernel::{BRep, BoolOp, SolidId};

/// Boolean operation of a feature with the existing body.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Operation {
    /// A separate solid body (files from before Booleans load as this).
    #[default]
    NewSolid,
    /// Added to the body.
    Join,
    /// Removed from the body.
    Cut,
    /// Only the common volume stays.
    Intersect,
}

impl Operation {
    /// Inventor's command name.
    pub fn label(self) -> &'static str {
        match self {
            Operation::NewSolid => "New Solid",
            Operation::Join => "Join",
            Operation::Cut => "Cut",
            Operation::Intersect => "Intersect",
        }
    }

    /// Default for a new feature: Join when there is a solid to join to.
    pub fn default_for(body: &BRep) -> Self {
        if body.solids().any(|(_, s)| !s.open) {
            Operation::Join
        } else {
            Operation::NewSolid
        }
    }
}

/// Solid or surface result.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Output {
    /// A closed solid.
    #[default]
    Solid,
    /// An open surface body (no caps), not combined with solids.
    Surface,
}

/// Combines the solids a feature just added to `body` (everything not in
/// `before`) with the solids that existed before, by `op`. New solids and
/// surfaces stay as they are. On failure `body` may be partly changed —
/// callers work on a copy.
/// `solids` limits the bodies operated on (by name; empty = all, Join
/// takes the first).
pub fn apply(
    body: &mut BRep,
    before: &HashSet<SolidId>,
    op: Operation,
    solids: &[String],
) -> Result<(), String> {
    let added: Vec<SolidId> = body
        .solids()
        .map(|(id, _)| id)
        .filter(|id| !before.contains(id))
        .collect();
    if op == Operation::NewSolid
        || added
            .iter()
            .any(|&id| body.solid(id).is_some_and(|s| s.open))
    {
        return Ok(());
    }
    let kop = match op {
        Operation::Join => BoolOp::Union,
        Operation::Cut => BoolOp::Subtract,
        Operation::Intersect => BoolOp::Intersect,
        Operation::NewSolid => return Ok(()), // handled above
    };
    let tool = body.clone();
    for &t in &added {
        body.remove_solid(t);
    }
    for &t in &added {
        let targets: Vec<SolidId> = body
            .solids()
            .filter(|(id, s)| !s.open && !added.contains(id))
            .filter(|(_, s)| {
                solids.is_empty() || s.name.as_ref().is_some_and(|n| solids.contains(n))
            })
            .map(|(id, _)| id)
            .collect();
        if targets.is_empty() {
            return Err(if solids.is_empty() {
                format!("{}: there is no solid body to operate on", op.label())
            } else {
                format!("{}: the chosen body no longer exists", op.label())
            });
        }
        match op {
            Operation::Join => {
                // With the first body (Inventor: the chosen body); not
                // touching it, the feature becomes a separate lump.
                body.boolean(targets[0], &tool, t, kop)
                    .map_err(|e| format!("Join failed: {e}"))?;
            }
            Operation::Cut => {
                let mut hit = false;
                for &target in &targets {
                    match body.boolean(target, &tool, t, kop) {
                        Ok(_) => hit = true,
                        Err(e) if e.to_string().contains("does not intersect") => {}
                        Err(e) => {
                            // Everything cut away: the body disappears.
                            if e.to_string().contains("empty") {
                                body.remove_solid(target);
                                hit = true;
                            } else {
                                return Err(format!("Cut failed: {e}"));
                            }
                        }
                    }
                }
                if !hit {
                    return Err("Cut: the feature does not intersect the body".into());
                }
            }
            Operation::Intersect => {
                let mut kept = false;
                for &target in &targets {
                    match body.boolean(target, &tool, t, kop) {
                        Ok(_) => kept = true,
                        Err(_) => body.remove_solid(target),
                    }
                }
                if !kept {
                    return Err("Intersect: the feature does not intersect the body".into());
                }
            }
            Operation::NewSolid => {}
        }
    }
    Ok(())
}
