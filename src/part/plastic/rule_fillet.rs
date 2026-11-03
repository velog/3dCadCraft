//! Rule Fillet (`docs/spec/5.8-plastic-parts.md`): rows of *source, rule, radius*. Edges are found
//! at every regeneration from the owners of the faces they separate, never stored, so the fillet
//! follows the model; a row that matches nothing is skipped.

use serde::{Deserialize, Serialize};

use super::common;
use super::is_false;
use crate::kernel::exact::ExactSolid;
use crate::kernel::{BRep, EdgeId, FaceId, Real, TessellationOptions, Vector3};
use crate::part::blend::edge_ref;
use crate::part::exact::Fail;
use crate::part::placement::WorkGeom;
use crate::part::select::{edge_convex, edges_of_face, feature_id_of_face};
use crate::part::shell::FaceRef;
use crate::part::work::LineRef;

fn yes() -> bool {
    true
}

/// What a row starts from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum RuleSource {
    /// Features, by id.
    Features(Vec<String>),
    /// Faces.
    Faces(Vec<FaceRef>),
}

/// Which edges of the source a row takes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FilletRule {
    /// Every edge of the source.
    AllEdges,
    /// Features: where their faces meet the rest of the part.
    AgainstPart,
    /// Features: edges between their own faces.
    FreeEdges,
    /// Where the source meets the features of the scope.
    AgainstFeatures(Vec<String>),
    /// Faces: straight edges leaving them along a direction.
    IncidentEdges {
        /// The direction.
        axis: LineRef,
        /// Against that line's own direction.
        #[serde(default, skip_serializing_if = "is_false")]
        flip: bool,
        /// How far from it an edge may lean (radians).
        tolerance: Real,
    },
}

/// A row of the table.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RuleRow {
    /// Features or faces.
    pub source: RuleSource,
    /// The rule.
    pub rule: FilletRule,
    /// The radius.
    pub radius: Real,
    /// Takes convex edges.
    #[serde(default = "yes")]
    pub all_rounds: bool,
    /// Takes concave edges.
    #[serde(default = "yes")]
    pub all_fillets: bool,
}

impl RuleRow {
    /// A row of features with both kinds of edges.
    pub fn features(ids: Vec<String>, rule: FilletRule, radius: Real) -> Self {
        Self {
            source: RuleSource::Features(ids),
            rule,
            radius,
            all_rounds: true,
            all_fillets: true,
        }
    }
}

/// Rule Fillet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RuleFilletOp {
    /// The rows; an edge goes with the first row that takes it.
    pub rows: Vec<RuleRow>,
    /// Faces whose edges stay as they are.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude_faces: Vec<String>,
    /// Remove Merged Faces: a source feature's face flush with the rest of the part is left out.
    #[serde(default, skip_serializing_if = "is_false")]
    pub remove_merged: bool,
}

impl RuleFilletOp {
    /// Refuses a definition that cannot be built (before any geometry).
    pub fn validate(&self) -> Result<(), String> {
        if self.rows.is_empty() {
            return Err("add a rule".into());
        }
        for r in &self.rows {
            let empty = match &r.source {
                RuleSource::Features(v) => v.is_empty(),
                RuleSource::Faces(v) => v.is_empty(),
            };
            if empty {
                return Err("select the source of every rule".into());
            }
            if r.radius <= 0.0 {
                return Err("the radius must be positive".into());
            }
            if !r.all_rounds && !r.all_fillets {
                return Err("a rule must take rounds, fillets or both".into());
            }
            match (&r.source, &r.rule) {
                (RuleSource::Features(_), FilletRule::IncidentEdges { .. }) => {
                    return Err("Incident Edges starts from faces".into());
                }
                (RuleSource::Faces(_), FilletRule::AgainstPart | FilletRule::FreeEdges) => {
                    return Err("this rule starts from features".into());
                }
                (_, FilletRule::AgainstFeatures(v)) if v.is_empty() => {
                    return Err("Against Features needs the features of the scope".into());
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// The browser's tooltip.
    pub fn describe(&self) -> String {
        format!("Rule Fillet, {} rule(s)", self.rows.len())
    }
}

fn owner(body: &BRep, f: FaceId) -> Option<String> {
    let n = body.face(f)?.name.as_deref()?;
    feature_id_of_face(n).map(str::to_owned)
}

/// The faces a row's source names (a face and its smooth group).
fn source_faces(body: &BRep, faces: &[FaceRef]) -> Vec<FaceId> {
    let mut out = Vec::new();
    for fr in faces {
        for (id, f) in body.faces() {
            if f.name.as_deref() == Some(fr.face.as_str()) {
                for g in body.smooth_group(id) {
                    if !out.contains(&g) {
                        out.push(g);
                    }
                }
            }
        }
    }
    out
}

/// Planar faces of the features `ids` lying in one plane with a face of something else.
fn merged_faces(body: &BRep, ids: &[String]) -> Vec<FaceId> {
    use crate::kernel::Surface;
    let plane = |f: FaceId| match body.face(f).and_then(|x| body.surface(x.surface)) {
        Some(Surface::Plane(p)) => Some(p.clone()),
        _ => None,
    };
    let mine = |f: FaceId| owner(body, f).is_some_and(|o| ids.contains(&o));
    let all: Vec<FaceId> = body.faces().map(|(f, _)| f).collect();
    all.iter()
        .copied()
        .filter(|&f| {
            let Some(p) = plane(f).filter(|_| mine(f)) else {
                return false;
            };
            all.iter().any(|&g| {
                !mine(g)
                    && plane(g).is_some_and(|q| {
                        q.normal.dot(&p.normal).abs() > 1.0 - 1e-9
                            && (q.origin - p.origin).dot(&p.normal).abs() < 1e-6
                    })
            })
        })
        .collect()
}

/// Does the straight edge `e` leave `faces` along `axis`?
fn incident(body: &BRep, e: EdgeId, faces: &[FaceId], axis: Vector3, tolerance: Real) -> bool {
    let Some(edge) = body.edge(e) else {
        return false;
    };
    let on = |v| {
        faces.iter().any(|&f| {
            edges_of_face(body, f)
                .iter()
                .any(|&x| body.edge(x).is_some_and(|y| y.start == v || y.end == v))
        })
    };
    let Ok(pts) = body.sample_edge(e, &TessellationOptions::default()) else {
        return false;
    };
    let (Some(a), Some(b)) = (pts.first(), pts.last()) else {
        return false;
    };
    // From the end on the face to the other one.
    let dir = if on(edge.start) {
        b - a
    } else if on(edge.end) {
        a - b
    } else {
        return false;
    };
    dir.try_normalize(1e-12)
        .is_some_and(|d| d.dot(&axis) >= tolerance.cos() - 1e-12)
}

/// Does `row` take the edge `e` between `f1` and `f2`?
fn takes(
    body: &BRep,
    row: &RuleRow,
    e: EdgeId,
    (f1, f2): (FaceId, FaceId),
    faces: &[FaceId],
    axis: Option<Vector3>,
) -> bool {
    let (o1, o2) = (owner(body, f1), owner(body, f2));
    let is_in = |o: &Option<String>, v: &[String]| o.as_ref().is_some_and(|x| v.contains(x));
    let on_face = |f: FaceId| faces.contains(&f);
    match (&row.source, &row.rule) {
        (RuleSource::Features(s), FilletRule::AllEdges) => is_in(&o1, s) || is_in(&o2, s),
        (RuleSource::Features(s), FilletRule::AgainstPart) => is_in(&o1, s) != is_in(&o2, s),
        (RuleSource::Features(s), FilletRule::FreeEdges) => is_in(&o1, s) && is_in(&o2, s),
        (RuleSource::Features(s), FilletRule::AgainstFeatures(t)) => {
            (is_in(&o1, s) && is_in(&o2, t)) || (is_in(&o2, s) && is_in(&o1, t))
        }
        (RuleSource::Faces(_), FilletRule::AllEdges) => on_face(f1) || on_face(f2),
        (RuleSource::Faces(_), FilletRule::AgainstFeatures(t)) => {
            (on_face(f1) && is_in(&o2, t)) || (on_face(f2) && is_in(&o1, t))
        }
        (RuleSource::Faces(_), FilletRule::IncidentEdges { tolerance, .. }) => {
            !on_face(f1)
                && !on_face(f2)
                && axis.is_some_and(|a| incident(body, e, faces, a, *tolerance))
        }
        _ => false,
    }
}

/// Builds Rule Fillet (`cur`: the solids before it).
pub(in crate::part) fn rule_fillet(
    op: &RuleFilletOp,
    id: &str,
    body: &BRep,
    works: &WorkGeom,
    cur: &[ExactSolid],
) -> Result<Vec<ExactSolid>, Fail> {
    let mut left_out: Vec<FaceId> = body
        .faces()
        .filter(|(_, f)| f.name.as_ref().is_some_and(|n| op.exclude_faces.contains(n)))
        .map(|(f, _)| f)
        .collect();
    if op.remove_merged {
        for r in &op.rows {
            if let RuleSource::Features(ids) = &r.source {
                left_out.extend(merged_faces(body, ids));
            }
        }
    }
    let mut rows = Vec::with_capacity(op.rows.len());
    for r in &op.rows {
        let faces = match &r.source {
            RuleSource::Faces(v) => source_faces(body, v),
            RuleSource::Features(_) => Vec::new(),
        };
        let axis = match &r.rule {
            FilletRule::IncidentEdges { axis, flip, .. } => {
                let (_, d) = axis
                    .resolve(body, works)
                    .map_err(|e| Fail::User(format!("Incident Edges: {e}")))?;
                Some(if *flip { -d } else { d })
            }
            _ => None,
        };
        rows.push((r, faces, axis));
    }
    let mut chosen = Vec::new();
    for e in body.visible_edges() {
        let fs = body.edge_faces(e);
        let [f1, f2] = fs[..] else {
            continue;
        };
        if left_out.contains(&f1) || left_out.contains(&f2) {
            continue;
        }
        let Some((row, ..)) = rows
            .iter()
            .find(|(r, faces, axis)| takes(body, r, e, (f1, f2), faces, *axis))
        else {
            continue;
        };
        // Smooth edges have nothing to round; the filter asks for one kind or both.
        let Some(convex) = edge_convex(body, e) else {
            continue;
        };
        if (convex && !row.all_rounds) || (!convex && !row.all_fillets) {
            continue;
        }
        if let Some(rf) = edge_ref(body, e) {
            chosen.push((rf, row.radius));
        }
    }
    common::apply_fillets(cur.to_vec(), body, &chosen, id)
}
