//! Plastic part features (`docs/spec/5.8-plastic-parts.md`): Grill, Boss, Rest, Snap Fit, Rule
//! Fillet and Lip, each a [`BodyOp`](super::BodyOp) variant built with tools and Booleans.

pub mod boss;
pub(super) mod common;
pub mod grill;
pub mod lip;
pub mod rest;
pub mod rule_fillet;
pub mod snap_fit;

use serde::{Deserialize, Serialize};

use super::placement::{PointRef, WorkGeom};
use super::work::LineRef;
use crate::kernel::{BRep, Point3, Real, Vector3};
use crate::sketch::{PointId, PointStyle};

/// The places of Boss and Snap Fit: a sketch's points, or model points with a direction.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Centers {
    /// From Sketch: points of one sketch; empty = its Center Points.
    Sketch {
        /// The sketch.
        sketch: String,
        /// Its points.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        points: Vec<PointId>,
    },
    /// On Point: model points, and the direction they share.
    Points {
        /// The points.
        points: Vec<PointRef>,
        /// The direction.
        direction: LineRef,
    },
}

impl Centers {
    /// The sketch it reads.
    pub fn sketch(&self) -> Option<&str> {
        match self {
            Centers::Sketch { sketch, .. } => Some(sketch),
            Centers::Points { .. } => None,
        }
    }

    pub(super) fn validate(&self) -> Result<(), String> {
        match self {
            Centers::Sketch { sketch, .. } if sketch.is_empty() => {
                Err("select the centres' sketch".into())
            }
            Centers::Points { points, .. } if points.is_empty() => {
                Err("select the centre points".into())
            }
            _ => Ok(()),
        }
    }

    /// The points in model space, the direction along which they work (the sketch's normal or
    /// the chosen line) and a direction square to it (the sketch's X).
    pub(super) fn resolve(
        &self,
        body: &BRep,
        works: &WorkGeom,
        sketches: &[super::PartSketch],
    ) -> Result<(Vec<Point3>, Vector3, Vector3), String> {
        match self {
            Centers::Sketch { sketch, points } => {
                let ps = sketches
                    .iter()
                    .find(|s| &s.id == sketch)
                    .ok_or("the sketch no longer exists")?;
                let sk = &ps.sketch;
                let mut chosen: Vec<PointId> = if points.is_empty() {
                    sk.points()
                        .filter(|(_, p)| p.style == PointStyle::Center)
                        .map(|(id, _)| id)
                        .collect()
                } else {
                    points.clone()
                };
                chosen.retain(|id| sk.point(*id).is_some());
                if chosen.is_empty() {
                    return Err("the sketch has no centre points".into());
                }
                let pts = chosen
                    .iter()
                    .filter_map(|id| sk.point(*id))
                    .map(|p| sk.plane.point_at(p.position.x, p.position.y))
                    .collect();
                Ok((
                    pts,
                    sk.plane.normal.into_inner(),
                    sk.plane.x_axis.into_inner(),
                ))
            }
            Centers::Points { points, direction } => {
                let (_, d) = direction.resolve(body, works).map_err(|e| e.to_string())?;
                let mut pts = Vec::with_capacity(points.len());
                for p in points {
                    pts.push(
                        super::placement::point_of(p, body, works).map_err(|e| e.to_string())?,
                    );
                }
                // Any direction square to d: the axis it is least along.
                let axis = [Vector3::x(), Vector3::y(), Vector3::z()]
                    .into_iter()
                    .min_by(|a, b| a.dot(&d).abs().total_cmp(&b.dot(&d).abs()))
                    .unwrap_or_else(Vector3::x);
                let x = (axis - d * axis.dot(&d))
                    .try_normalize(1e-12)
                    .ok_or("the direction is not valid")?;
                Ok((pts, d, x))
            }
        }
    }
}

pub(super) fn is_zero(v: &Real) -> bool {
    *v == 0.0
}

pub(super) fn is_false(v: &bool) -> bool {
    !*v
}
