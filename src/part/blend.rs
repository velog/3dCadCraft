//! Fillet and Chamfer of model edges (`docs/spec/2.4-fillet.md`,
//! `docs/spec/2.5-chamfer.md`).
//!
//! Stage 1 builds every treated edge as a Boolean with a *tool* body:
//! - a straight edge between two planar faces: the region between the two
//!   faces and the fillet arc (or chamfer line), drawn in the plane across
//!   the edge, extruded along it;
//! - a circular edge between a planar face and a cylinder (exact or
//!   faceted): the same region in a plane through the cylinder axis,
//!   revolved around it.
//!
//! A convex edge loses the region (Cut), a concave one gains it (Join).
//! Edges are referenced by the names of their two faces plus a hint point
//! ([`EdgeRef`]), which survives regeneration and Booleans (rule 2).

use std::f64::consts::{PI, TAU};

use serde::{Deserialize, Serialize};

use super::params::FeatureParams;
use crate::kernel::{
    BRep, BoolOp, EdgeId, ExtrudeLevel, FaceId, Plane, Point3, ProfileSegment, Real, RevolveAxis,
    Segment2, Sense, SolidId, Surface, Vector3,
};

type P2 = nalgebra::Point2<Real>;
type V2 = nalgebra::Vector2<Real>;

/// A model edge, by the persistent names of its two faces and a point near
/// it (for when the two faces meet along several edges).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EdgeRef {
    /// Names of the two faces the edge separates.
    pub faces: [String; 2],
    /// A point on the edge when it was picked (model coordinates).
    pub hint: [Real; 3],
}

/// A further constant-radius edge set of a fillet (Inventor: one row per
/// set in the Constant tab; the first set is [`BlendFeature::edges`] with
/// the kind's radius).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FilletSet {
    /// The set's edges.
    pub edges: Vec<EdgeRef>,
    /// Its radius (mm); parameter field `radius<k+2>`.
    pub radius: Real,
}

/// A variable-radius edge (Inventor: Variable tab): the radius at points
/// along a straight edge, from its end nearest `start`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VariableRadius {
    /// The edge.
    pub edge: EdgeRef,
    /// A point at (or near) the edge's start (model coordinates).
    pub start: [Real; 3],
    /// (position 0..1 from the start, radius mm); 0 and 1 always present.
    pub points: Vec<[Real; 2]>,
}

/// A Partial chamfer (Inventor): the chamfer covers only part of its edge,
/// `to_start` from the edge's end nearest `start` and `length` long; its
/// ends are planes square to the edge.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PartialChamfer {
    /// A point at (or near) the end the distances are measured from.
    pub start: [Real; 3],
    /// From that end to the chamfer (mm, ≥ 0; Inventor's To Start).
    pub to_start: Real,
    /// The chamfer's length along the edge (mm; Inventor's Chamfer).
    pub length: Real,
}

/// Fillet types beyond the constant edge fillet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilletType {
    /// Constant radius per set (the default).
    #[default]
    Constant,
    /// The radius varies along each edge ([`BlendFeature::variable`]).
    Variable,
    /// Constant chord (blend width across the edge) instead of a radius:
    /// edges with different face angles get the same width (Creo's
    /// Chordal, SolidWorks' chord width). The kind's radius is the chord.
    Chord,
    /// Full Round: the middle face between two parallel side faces is
    /// replaced by a round touching all three ([`BlendFeature::full_round`]);
    /// the radius is half the distance between the sides.
    FullRound,
}

/// Chamfer methods (Inventor's).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChamferMethod {
    /// The same distance on both faces.
    #[default]
    Distance,
    /// A distance on each face (Flip swaps them).
    TwoDistances,
    /// A distance on the first face and an angle from it.
    DistanceAngle,
}

/// What is done to the edges.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum BlendKind {
    /// Constant-radius fillet.
    Fillet {
        /// Radius (mm).
        radius: Real,
    },
    /// Chamfer.
    Chamfer {
        /// Method.
        method: ChamferMethod,
        /// Distance (mm), the first one for Two Distances.
        distance: Real,
        /// Second distance (mm, Two Distances).
        distance2: Real,
        /// Angle (radians, Distance and Angle).
        angle: Real,
        /// Swap the faces the distances apply to.
        flip: bool,
    },
}

/// A Fillet or Chamfer feature.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BlendFeature {
    /// Immutable identity (`Fillet1`, `Chamfer1`): topology-name prefix.
    pub id: String,
    /// Display name.
    pub name: String,
    /// The treated edges.
    pub edges: Vec<EdgeRef>,
    /// Fillet radius or chamfer definition.
    pub kind: BlendKind,
    /// Equations of the numeric fields.
    #[serde(default, skip_serializing_if = "FeatureParams::is_empty")]
    pub params: FeatureParams,
    /// **Cone at small rounds** (`docs/spec/2.5-chamfer.md` §2b): where the
    /// blend is larger than an arc of the loop, follow the arc as a cone up
    /// to its apex; off: the neighbouring planes run on through the corner
    /// and cut the round.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cone_corners: bool,
    /// Fillet type (Constant, Variable, Chord, Full Round).
    #[serde(default, skip_serializing_if = "is_constant")]
    pub fillet_type: FilletType,
    /// Further constant-radius sets (Constant).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sets: Vec<FilletSet>,
    /// Variable-radius edges (Variable).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variable: Vec<VariableRadius>,
    /// Smooth Radius Transition (Variable): C¹ between the points, else
    /// straight.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub smooth: bool,
    /// The middle face of a Full Round (persistent name).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_round: Option<String>,
    /// Partial chamfer of its (one) edge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partial: Option<PartialChamfer>,
    /// The preset the feature was made with (`docs/spec/0.11-presets.md`):
    /// only its name, Edit Feature shows it again; the values are the
    /// feature's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
}

fn is_constant(t: &FilletType) -> bool {
    *t == FilletType::Constant
}

impl BlendFeature {
    /// A fillet of `edges` with `radius`.
    pub fn fillet(edges: Vec<EdgeRef>, radius: Real) -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            edges,
            kind: BlendKind::Fillet { radius },
            params: FeatureParams::default(),
            cone_corners: false,
            fillet_type: FilletType::Constant,
            sets: Vec::new(),
            variable: Vec::new(),
            smooth: false,
            full_round: None,
            partial: None,
            preset: None,
        }
    }

    /// An equal-distance chamfer of `edges`.
    pub fn chamfer(edges: Vec<EdgeRef>, distance: Real) -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            edges,
            kind: BlendKind::Chamfer {
                method: ChamferMethod::Distance,
                distance,
                distance2: distance,
                angle: PI / 4.0,
                flip: false,
            },
            params: FeatureParams::default(),
            cone_corners: false,
            fillet_type: FilletType::Constant,
            sets: Vec::new(),
            variable: Vec::new(),
            smooth: false,
            full_round: None,
            partial: None,
            preset: None,
        }
    }

    /// A variable-radius fillet of straight `edges` (start, points each).
    pub fn variable_fillet(variable: Vec<VariableRadius>, smooth: bool) -> Self {
        let r = variable
            .first()
            .and_then(|v| v.points.first())
            .map_or(1.0, |p| p[1]);
        let mut f = Self::fillet(Vec::new(), r);
        f.fillet_type = FilletType::Variable;
        f.variable = variable;
        f.smooth = smooth;
        f
    }

    /// A chord-width fillet of `edges`.
    pub fn chord_fillet(edges: Vec<EdgeRef>, chord: Real) -> Self {
        let mut f = Self::fillet(edges, chord);
        f.fillet_type = FilletType::Chord;
        f
    }

    /// A Full Round of the face named `center` (between two parallel side
    /// faces).
    pub fn full_round(center: impl Into<String>) -> Self {
        let mut f = Self::fillet(Vec::new(), 1.0);
        f.fillet_type = FilletType::FullRound;
        f.full_round = Some(center.into());
        f
    }

    /// Every edge the feature uses (all sets, the variable edges).
    pub fn all_edges(&self) -> Vec<EdgeRef> {
        let mut v = self.edges.clone();
        for s in &self.sets {
            v.extend(s.edges.iter().cloned());
        }
        v.extend(self.variable.iter().map(|x| x.edge.clone()));
        v
    }

    /// Whether anything is selected to blend.
    pub fn has_input(&self) -> bool {
        match self.fillet_type {
            FilletType::FullRound => self.full_round.is_some(),
            FilletType::Variable => !self.variable.is_empty(),
            _ => !self.edges.is_empty() || self.sets.iter().any(|s| !s.edges.is_empty()),
        }
    }

    /// `"Fillet"` or `"Chamfer"`.
    pub fn command(&self) -> &'static str {
        match self.kind {
            BlendKind::Fillet { .. } => "Fillet",
            BlendKind::Chamfer { .. } => "Chamfer",
        }
    }

    /// Applies the feature to `body`. Edges that no longer exist are
    /// skipped; it fails when none is left or a Boolean fails.
    pub fn build(&self, body: &mut BRep) -> Result<(), String> {
        if matches!(self.kind, BlendKind::Fillet { .. })
            && (self.fillet_type != FilletType::Constant || !self.sets.is_empty())
            || self.partial.is_some()
        {
            return Err("this fillet type needs the exact kernel".into());
        }
        // Every tool is made from the body *before* the feature, so the
        // edges keep their full length where treated edges meet.
        let original = body.clone();
        let mut first_err = None;
        // Whole outer loops of planar faces (walls perpendicular): one
        // levelled tool each, applied after the single edges.
        let (loops, singles) = split_loops(&original, &self.edges);
        let mut tools = Vec::new();
        for (k, e) in singles.iter().enumerate() {
            let prefix = if k == 0 {
                self.id.clone()
            } else {
                format!("{}#{}", self.id, k + 1)
            };
            match self.make_tool(&original, e, &prefix) {
                Ok(t) => tools.push(t),
                Err(msg) => {
                    first_err.get_or_insert(msg);
                }
            }
        }
        let mut done = 0;
        for (tool, solid, target, convex, nudge) in tools {
            let op = if convex {
                BoolOp::Subtract
            } else {
                BoolOp::Union
            };
            // The body by its persistent name (ids change with every
            // Boolean); several pieces: each one the tool touches.
            let targets: Vec<SolidId> = body
                .solids()
                .filter(|(_, s)| !s.open && s.name == target)
                .map(|(id, _)| id)
                .collect();
            let mut hit = false;
            for t in targets {
                // Tangent contact can leave degenerate slivers; a tool moved
                // 1 µm into the material avoids the exact tangency.
                let first = body.boolean(t, &tool, solid, op);
                let result = match first {
                    Err(e) if !e.to_string().contains("does not intersect") => {
                        let mut moved = tool.clone();
                        moved.transform(&crate::kernel::Transform3::translation(
                            nudge.x * 1e-3,
                            nudge.y * 1e-3,
                            nudge.z * 1e-3,
                        ));
                        body.boolean(t, &moved, solid, op)
                    }
                    other => other,
                };
                match result {
                    Ok(_) => hit = true,
                    Err(e) if e.to_string().contains("does not intersect") => {}
                    Err(e) => {
                        first_err.get_or_insert(format!("{}: {e}", self.command()));
                    }
                }
            }
            if hit {
                done += 1;
            }
        }
        for (k, cap) in loops.iter().enumerate() {
            let prefix = format!("{}#L{}", self.id, k + 1);
            match self.apply_loop(body, cap, &prefix) {
                Ok(()) => done += 1,
                Err(msg) => {
                    first_err.get_or_insert(format!("{}: {msg}", self.command()));
                }
            }
        }
        if done == 0 {
            return Err(first_err.unwrap_or_else(|| "no edges selected".into()));
        }
        if let Some(e) = first_err
            && !e.contains("no longer exists")
        {
            return Err(e);
        }
        Ok(())
    }

    /// The cross-section of the treatment for a 90° convex edge as
    /// (depth below the cap, inward distance from the wall) pairs, from the
    /// wall (inset 0) to the cap.
    fn section(&self) -> Result<Vec<(Real, Real)>, String> {
        match &self.kind {
            BlendKind::Fillet { radius } => {
                let r = *radius;
                if r <= 0.0 {
                    return Err("the radius must be positive".into());
                }
                const N: usize = 16;
                // Not the exact tangent ends: the section there is (almost)
                // parallel to the wall and the cap, and faceted near-parallel
                // surfaces break the Boolean. The step left is r·(1 − cos(π/2N)).
                Ok((1..N)
                    .map(|k| {
                        let phi = PI / 2.0 * k as Real / N as Real;
                        (r - r * phi.sin(), r - r * phi.cos())
                    })
                    .collect())
            }
            BlendKind::Chamfer { .. } => {
                let (d1, d2) = self.setbacks(PI / 2.0);
                if d1 <= 0.0 || d2 <= 0.0 {
                    return Err("the distances must be positive".into());
                }
                // d1 along the cap (inward), d2 down the wall.
                Ok(vec![(d2, 0.0), (0.0, d1)])
            }
        }
    }

    /// Treats the whole outer loop of the planar face `cap` (walls
    /// perpendicular to it): the loop is extruded through levels that
    /// follow the section (tapered), and everything outside that, near the
    /// cap, is cut away. Arcs in the loop (earlier fillets) are followed
    /// exactly, so corners come out spherical / conical.
    fn apply_loop(&self, body: &mut BRep, cap: &str, prefix: &str) -> Result<(), String> {
        let lp = cap_loop(body, cap)?;
        let section = self.section()?;
        let depth = section[0].0;
        let margin = (depth * 0.25).max(0.3);
        let n = lp.plane.normal.into_inner();
        // How far the body reaches below the cap.
        let below = body
            .vertices()
            .map(|(_, v)| (lp.plane.origin - v.point).dot(&n))
            .fold(0.0, Real::max);
        // Arcs shrink by the inset: keep them a hair above zero radius.
        let max_inset = lp.min_arc - 1e-3 * lp.min_arc.max(1.0);
        // The kept region: wider than the body below the treatment (walls
        // never coincide), the section near the cap, and on above it.
        let mut levels = vec![
            ExtrudeLevel {
                offset: -(below + margin).max(depth + 2.0 * margin),
                inset: margin,
            },
            ExtrudeLevel {
                offset: -(depth + 0.5 * margin),
                inset: margin,
            },
        ];
        let mut last = 0.0;
        for &(h, s) in &section {
            last = -s.min(max_inset);
            levels.push(ExtrudeLevel {
                offset: -h,
                inset: last,
            });
        }
        levels.push(ExtrudeLevel {
            offset: margin,
            inset: last,
        });
        let mut keep = BRep::new();
        let keep_id = keep
            .extrude_profile(&lp.plane, &lp.segments, &[], &levels, &format!("{prefix}K"))
            .map_err(|e| e.to_string())?;
        // Tool = a frame around the loop near the cap, minus the kept part.
        let frame = [
            ExtrudeLevel {
                offset: -(depth + 0.5 * margin),
                inset: margin,
            },
            ExtrudeLevel {
                offset: 0.5 * margin,
                inset: margin,
            },
        ];
        let mut tool = BRep::new();
        let frame_id = tool
            .extrude_profile(&lp.plane, &lp.segments, &[], &frame, prefix)
            .map_err(|e| e.to_string())?;
        let ids = tool
            .boolean(frame_id, &keep, keep_id, BoolOp::Subtract)
            .map_err(|e| e.to_string())?;
        let target = body
            .solids()
            .find(|(_, s)| s.shells.iter().any(|sh| sh.faces.contains(&lp.face)))
            .map(|(id, _)| id)
            .ok_or("the face has no solid")?;
        let name = body.solid(target).and_then(|s| s.name.clone());
        for t in ids {
            let targets: Vec<SolidId> = body
                .solids()
                .filter(|(_, s)| !s.open && s.name == name)
                .map(|(id, _)| id)
                .collect();
            for b in targets {
                match body.boolean(b, &tool, t, BoolOp::Subtract) {
                    Ok(_) => {}
                    Err(e) if e.to_string().contains("does not intersect") => {}
                    Err(_) => {
                        // Tangent contact: retry 1 µm into the material.
                        let mut moved = tool.clone();
                        let d = -n * 1e-3;
                        moved.transform(&crate::kernel::Transform3::translation(d.x, d.y, d.z));
                        body.boolean(b, &moved, t, BoolOp::Subtract)
                            .map_err(|e| e.to_string())?;
                    }
                }
            }
        }
        if matches!(self.kind, BlendKind::Fillet { .. }) {
            self.smooth_loop_strips(body, &lp, prefix, section.len());
        }
        Ok(())
    }

    /// The fillet of a loop is made of flat / conical strips (one per pair
    /// of levels): mark the strips of each loop segment as facets of the
    /// exact round (a cylinder along a line, a torus or sphere around an
    /// arc), named `<prefix>:Round:<segment>`.
    fn smooth_loop_strips(&self, body: &mut BRep, lp: &CapLoop, prefix: &str, n_section: usize) {
        let BlendKind::Fillet { radius: r } = self.kind else {
            return;
        };
        let n = lp.plane.normal.into_inner();
        let side_prefix = format!("{prefix}K:Side");
        for seg in &lp.segments {
            // Strips between section levels: level indices 2 .. n_section.
            let strips: Vec<FaceId> = body
                .faces()
                .filter(|(_, f)| {
                    f.name.as_deref().is_some_and(|nm| {
                        let Some(rest) = nm.strip_prefix(&side_prefix) else {
                            return false;
                        };
                        let Some((lvl, sname)) = rest.split_once(':') else {
                            return false;
                        };
                        let lvl: usize = lvl.parse().unwrap_or(0);
                        sname == seg.name && (2..=n_section).contains(&lvl)
                    })
                })
                .map(|(id, _)| id)
                .collect();
            if strips.is_empty() {
                continue;
            }
            let p3 = |p: P2| lp.plane.point_at(p.x, p.y);
            let surface = match &seg.geometry {
                Segment2::Line { a, b } => {
                    let d = (b - a).normalize();
                    let inward = V2::new(-d.y, d.x); // left of a CCW loop
                    let axis_pt = p3(a + inward * r) - n * r;
                    let dir3 = p3(*b) - p3(*a);
                    crate::kernel::Cylinder::new(axis_pt, dir3, n, r)
                        .ok()
                        .map(Surface::Cylinder)
                }
                Segment2::Arc { c, r: big, .. } => {
                    let center = p3(*c) - n * r;
                    let x = lp.plane.x_axis;
                    let axis = lp.plane.normal;
                    if *big - r <= 1e-3 * r {
                        Some(Surface::Sphere(crate::kernel::Sphere {
                            center,
                            radius: r,
                            axis,
                            x_axis: x,
                        }))
                    } else {
                        Some(Surface::Torus(crate::kernel::Torus {
                            center,
                            axis,
                            x_axis: x,
                            major: big - r,
                            minor: r,
                        }))
                    }
                }
                Segment2::Spline { .. } => None,
            };
            if let Some(s) = surface {
                body.mark_facets(&strips, s, &format!("{prefix}:Round:{}", seg.name));
            }
        }
    }

    /// The tool body for one edge: (tool, its solid, target body name,
    /// convex edge).
    fn make_tool(
        &self,
        body: &BRep,
        e: &EdgeRef,
        prefix: &str,
    ) -> Result<(BRep, SolidId, Option<String>, bool, Vector3), String> {
        let geom = resolve(body, e)?;
        let (tool, solid, target, convex) = match &geom {
            EdgeGeom::Straight {
                a,
                b,
                u1,
                u2,
                reach,
                convex,
                target,
            } => {
                let d = (b - a).normalize();
                let profile = self.profile(*u1, *u2, d)?;
                // The tangent points must stay on the faces.
                let (s1, s2) = self.setbacks(u1.dot(u2).clamp(-1.0, 1.0).acos());
                if s1 > reach[0] + 1e-6 || s2 > reach[1] + 1e-6 {
                    return Err(format!(
                        "{}: the size is too large for the edge",
                        self.command()
                    ));
                }
                let ext = 0.02 * (b - a).norm().max(1.0);
                let plane =
                    Plane::new(a - d * ext, d, *u1).map_err(|e| format!("degenerate edge: {e}"))?;
                let segs = to_segments(&profile, &plane_basis_2d(&plane, u1, u2));
                let mut tool = BRep::new();
                let levels = [
                    ExtrudeLevel {
                        offset: 0.0,
                        inset: 0.0,
                    },
                    ExtrudeLevel {
                        offset: (b - a).norm() + 2.0 * ext,
                        inset: 0.0,
                    },
                ];
                let s = tool
                    .extrude_profile(&plane, &segs, &[], &levels, prefix)
                    .map_err(|e| e.to_string())?;
                (tool, s, *target, *convex)
            }
            EdgeGeom::Circular {
                center,
                axis,
                radial,
                radius,
                cap_inward,
                side_up,
                convex,
                target,
            } => {
                // Profile in the (radial, axial) half-plane through the axis.
                let u1 = V2::new(if *cap_inward { -1.0 } else { 1.0 }, 0.0);
                let u2 = V2::new(0.0, if *side_up { 1.0 } else { -1.0 });
                let local = self.profile_2d(u1, u2)?;
                let pts: Vec<Shape2> = local
                    .into_iter()
                    .map(|s| s.shifted(V2::new(*radius, 0.0)))
                    .collect();
                let normal = radial.cross(axis);
                let plane = Plane::new(*center, normal, *radial)
                    .map_err(|e| format!("degenerate edge: {e}"))?;
                let segs = to_segments(&pts, &Basis2::identity());
                let mut tool = BRep::new();
                let (start, sweep) = arc_range(body, e, center, axis, radial);
                let s = tool
                    .revolve_profile(
                        &plane,
                        &segs,
                        &[],
                        RevolveAxis {
                            origin: P2::origin(),
                            dir: V2::new(0.0, 1.0),
                        },
                        start,
                        sweep,
                        false,
                        prefix,
                    )
                    .map_err(|e| e.to_string())?;
                (tool, s, *target, *convex)
            }
        };
        let name = body.solid(target).and_then(|s| s.name.clone());
        // Into the corner of the material (for a retry, see `build`).
        let nudge = match &geom {
            EdgeGeom::Straight { u1, u2, .. } => -(u1 + u2).normalize(),
            EdgeGeom::Circular {
                axis,
                radial,
                cap_inward,
                side_up,
                ..
            } => {
                let r = if *cap_inward { -radial } else { *radial };
                let a = if *side_up { *axis } else { -axis };
                -(r + a).normalize()
            }
        };
        Ok((tool, solid, name, convex, nudge))
    }

    /// How far the treatment reaches along face 1 and face 2 from the edge,
    /// for a wedge angle `alpha`.
    fn setbacks(&self, alpha: Real) -> (Real, Real) {
        match &self.kind {
            BlendKind::Fillet { radius } => {
                let s = radius / (alpha / 2.0).tan();
                (s, s)
            }
            BlendKind::Chamfer {
                method,
                distance,
                distance2,
                angle,
                flip,
            } => {
                let (d1, d2) = match method {
                    ChamferMethod::Distance => (*distance, *distance),
                    ChamferMethod::TwoDistances => (*distance, *distance2),
                    ChamferMethod::DistanceAngle => {
                        let g = (PI - angle - alpha).max(1e-6);
                        (*distance, distance * angle.sin() / g.sin())
                    }
                };
                if *flip { (d2, d1) } else { (d1, d2) }
            }
        }
    }

    /// The cross-section of what the blend removes at a convex edge between
    /// a cap and a perpendicular wall: x runs inward along the cap, y down
    /// the wall, the edge is the origin (enlarged a little past both faces).
    /// The blend face's segment is named `face`.
    pub(super) fn wall_section(&self, face: &str) -> Result<Vec<ProfileSegment>, String> {
        let shapes = self.profile_2d(V2::new(1.0, 0.0), V2::new(0.0, 1.0))?;
        let mut segs = to_segments(&shapes, &Basis2::identity());
        for s in &mut segs {
            if s.name == "Face" {
                s.name = face.to_owned();
            }
        }
        Ok(segs)
    }

    /// How far the blend reaches from the edge into the cap and down a
    /// perpendicular wall (fillet: the radius twice).
    pub(super) fn wall_setbacks(&self) -> (Real, Real) {
        self.setbacks(PI / 2.0)
    }

    /// The region profile for a straight edge, in the 2D basis
    /// (`u1`, `u2` mapped into the plane across the edge).
    fn profile(&self, u1: Vector3, u2: Vector3, d: Vector3) -> Result<Vec<Shape2>, String> {
        let y = d.cross(&u1);
        let w2 = V2::new(u2.dot(&u1), u2.dot(&y));
        self.profile_2d(V2::new(1.0, 0.0), w2.normalize())
    }

    /// The region between the corner (origin), the two faces along unit
    /// directions `u1`, `u2` and the fillet arc / chamfer line, enlarged a
    /// little past the faces so the Boolean has no coincident faces.
    fn profile_2d(&self, u1: V2, u2: V2) -> Result<Vec<Shape2>, String> {
        let cos = u1.dot(&u2).clamp(-1.0, 1.0);
        let alpha = cos.acos(); // wedge angle between the faces
        if !(1e-3..=PI - 1e-3).contains(&alpha) {
            return Err("the faces are tangent at the edge".into());
        }
        let b = (u1 + u2).normalize();
        // Outward (away from the wedge) normals of the two face lines.
        let side = |u: V2, other: V2| {
            let p = V2::new(-u.y, u.x);
            if p.dot(&other) > 0.0 { -p } else { p }
        };
        let (o1, o2) = (side(u1, u2), side(u2, u1));
        let (t1, t2, arc) = match &self.kind {
            BlendKind::Fillet { radius } => {
                let r = *radius;
                if r <= 0.0 {
                    return Err("the radius must be positive".into());
                }
                let s = r / (alpha / 2.0).tan();
                let c = P2::from(b * (r / (alpha / 2.0).sin()));
                (P2::from(u1 * s), P2::from(u2 * s), Some((c, r)))
            }
            BlendKind::Chamfer {
                method,
                distance,
                distance2,
                angle,
                flip,
            } => {
                let (d1, d2) = match method {
                    ChamferMethod::Distance => (*distance, *distance),
                    ChamferMethod::TwoDistances => (*distance, *distance2),
                    ChamferMethod::DistanceAngle => {
                        // A line from the point at `distance` on face 1 at
                        // `angle` from it meets face 2 at d2 (sine rule).
                        let gamma = PI - angle - alpha;
                        if gamma <= 1e-6 || *angle <= 0.0 {
                            return Err("the chamfer angle does not fit the edge".into());
                        }
                        (*distance, distance * angle.sin() / gamma.sin())
                    }
                };
                let (d1, d2) = if *flip { (d2, d1) } else { (d1, d2) };
                if d1 <= 0.0 || d2 <= 0.0 {
                    return Err("the distances must be positive".into());
                }
                (P2::from(u1 * d1), P2::from(u2 * d2), None)
            }
        };
        let size = (t1.coords.norm() + t2.coords.norm()).max(1e-3);
        let e = 0.05 * size + 0.01;
        let corner = P2::from(-b * e);
        let mut out = vec![
            Shape2::Line(corner, t1 + o1 * e),
            Shape2::Line(t1 + o1 * e, t1),
        ];
        match arc {
            Some((c, r)) => {
                let a1 = (t1 - c).y.atan2((t1 - c).x);
                let a2 = (t2 - c).y.atan2((t2 - c).x);
                let mut sweep = (a2 - a1 + PI).rem_euclid(TAU) - PI;
                if sweep.abs() < 1e-12 {
                    sweep = 0.0;
                }
                out.push(Shape2::Arc {
                    c,
                    r,
                    a0: a1,
                    sweep,
                });
            }
            None => out.push(Shape2::Line(t1, t2)),
        }
        out.push(Shape2::Line(t2, t2 + o2 * e));
        out.push(Shape2::Line(t2 + o2 * e, corner));
        // Counter-clockwise.
        if signed_area(&out) < 0.0 {
            out = out.into_iter().rev().map(Shape2::reversed).collect();
        }
        Ok(out)
    }
}

/// A 2D profile piece (before naming).
#[derive(Clone, Copy, Debug)]
enum Shape2 {
    Line(P2, P2),
    Arc {
        c: P2,
        r: Real,
        a0: Real,
        sweep: Real,
    },
}

impl Shape2 {
    fn reversed(self) -> Self {
        match self {
            Shape2::Line(a, b) => Shape2::Line(b, a),
            Shape2::Arc { c, r, a0, sweep } => Shape2::Arc {
                c,
                r,
                a0: a0 + sweep,
                sweep: -sweep,
            },
        }
    }

    fn shifted(self, v: V2) -> Self {
        match self {
            Shape2::Line(a, b) => Shape2::Line(a + v, b + v),
            Shape2::Arc { c, r, a0, sweep } => Shape2::Arc {
                c: c + v,
                r,
                a0,
                sweep,
            },
        }
    }

    fn sample(&self) -> Vec<P2> {
        match *self {
            Shape2::Line(a, _) => vec![a],
            Shape2::Arc { c, r, a0, sweep } => (0..16)
                .map(|k| {
                    let t = a0 + sweep * k as Real / 16.0;
                    c + V2::new(t.cos(), t.sin()) * r
                })
                .collect(),
        }
    }
}

fn signed_area(s: &[Shape2]) -> Real {
    let pts: Vec<P2> = s.iter().flat_map(Shape2::sample).collect();
    (0..pts.len())
        .map(|i| {
            let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
            p.x * q.y - q.x * p.y
        })
        .sum::<Real>()
        / 2.0
}

/// Maps the profile's own 2D frame into the sketch plane's 2D frame.
struct Basis2 {
    x: V2,
    y: V2,
}

impl Basis2 {
    fn identity() -> Self {
        Self {
            x: V2::new(1.0, 0.0),
            y: V2::new(0.0, 1.0),
        }
    }

    fn map(&self, p: P2) -> P2 {
        P2::from(self.x * p.x + self.y * p.y)
    }

    fn map_angle(&self, a: Real) -> Real {
        let v = self.x * a.cos() + self.y * a.sin();
        v.y.atan2(v.x)
    }

    /// Whether the frame is mirrored (angles run the other way).
    fn mirrored(&self) -> bool {
        self.x.perp(&self.y) < 0.0
    }
}

/// The profile frame (x = u1, y = d × u1) in the coordinates of `plane`
/// (whose x axis is `u1` and normal `d`): identity unless the plane's y is
/// the opposite of d × u1.
fn plane_basis_2d(plane: &Plane, u1: &Vector3, _u2: &Vector3) -> Basis2 {
    let y = plane.normal.into_inner().cross(u1);
    let s = if y.dot(&plane.y_axis().into_inner()) >= 0.0 {
        1.0
    } else {
        -1.0
    };
    Basis2 {
        x: V2::new(1.0, 0.0),
        y: V2::new(0.0, s),
    }
}

fn to_segments(shapes: &[Shape2], basis: &Basis2) -> Vec<ProfileSegment> {
    let mut out: Vec<ProfileSegment> = shapes
        .iter()
        .enumerate()
        .map(|(k, s)| {
            let geometry = match *s {
                Shape2::Line(a, b) => Segment2::Line {
                    a: basis.map(a),
                    b: basis.map(b),
                },
                Shape2::Arc { c, r, a0, sweep } => Segment2::Arc {
                    c: basis.map(c),
                    r,
                    a0: basis.map_angle(a0),
                    sweep: if basis.mirrored() { -sweep } else { sweep },
                },
            };
            let name = match s {
                Shape2::Arc { .. } => "Face".to_owned(),
                Shape2::Line(..) if k == 2 => "Face".to_owned(),
                _ => format!("S{k}"),
            };
            ProfileSegment { geometry, name }
        })
        .collect();
    // A mirrored frame reverses the loop's turning sense.
    if basis.mirrored() {
        out.reverse();
        for s in &mut out {
            s.geometry = match &s.geometry {
                Segment2::Line { a, b } => Segment2::Line { a: *b, b: *a },
                Segment2::Arc { c, r, a0, sweep } => Segment2::Arc {
                    c: *c,
                    r: *r,
                    a0: a0 + sweep,
                    sweep: -sweep,
                },
                other => other.clone(),
            };
        }
    }
    out
}

/// A resolved edge.
enum EdgeGeom {
    /// A straight edge `a → b` between two planar faces; `u1`, `u2` point
    /// from the edge into the faces (⟂ the edge).
    Straight {
        a: Point3,
        b: Point3,
        u1: Vector3,
        u2: Vector3,
        /// How far each face reaches from the edge (along `u1` / `u2`).
        reach: [Real; 2],
        convex: bool,
        target: SolidId,
    },
    /// A circular edge between a planar cap and a cylinder.
    Circular {
        center: Point3,
        axis: Vector3,
        radial: Vector3,
        radius: Real,
        /// The cap runs towards the axis from the edge.
        cap_inward: bool,
        /// The cylinder runs along +axis from the edge.
        side_up: bool,
        convex: bool,
        target: SolidId,
    },
}

/// The faces with persistent name `n`.
fn faces_named(body: &BRep, n: &str) -> Vec<FaceId> {
    body.faces()
        .filter(|(_, f)| f.name.as_deref() == Some(n))
        .map(|(id, _)| id)
        .collect()
}

/// The exact cylinder a face lies on (its surface, or the one its facets
/// approximate).
pub(super) fn cylinder_of(body: &BRep, f: FaceId) -> Option<crate::kernel::Cylinder> {
    let face = body.face(f)?;
    let s = face.facet_of.unwrap_or(face.surface);
    match body.surface(s)? {
        Surface::Cylinder(c) => Some(c.clone()),
        _ => None,
    }
}

/// Unit direction of an edge as the face `f` runs along it (its loops keep
/// the face on the left, seen from outside).
fn run_dir(body: &BRep, f: FaceId, e: EdgeId) -> Option<Vector3> {
    let face = body.face(f)?;
    let edge = body.edge(e)?;
    let (a, b) = (body.vertex(edge.start)?.point, body.vertex(edge.end)?.point);
    for lp in std::iter::once(&face.outer).chain(&face.inner) {
        for ce in &lp.coedges {
            if ce.edge == e {
                let d = (b - a).normalize();
                return Some(if ce.sense == Sense::Same { d } else { -d });
            }
        }
    }
    None
}

fn solid_of(body: &BRep, f: FaceId) -> Option<SolidId> {
    body.solids()
        .find(|(_, s)| s.shells.iter().any(|sh| sh.faces.contains(&f)))
        .map(|(id, _)| id)
}

fn resolve(body: &BRep, r: &EdgeRef) -> Result<EdgeGeom, String> {
    let fa = faces_named(body, &r.faces[0]);
    let fb = faces_named(body, &r.faces[1]);
    if fa.is_empty() || fb.is_empty() {
        return Err("the edge no longer exists".into());
    }
    let hint = Point3::from(r.hint);
    // Edges shared by a face of each name, nearest to the hint first.
    let mut shared: Vec<(Real, EdgeId, FaceId, FaceId)> = Vec::new();
    for (e, _) in body.edges() {
        let fs = body.edge_faces(e);
        let a = fs.iter().copied().find(|f| fa.contains(f));
        let b = fs.iter().copied().find(|f| fb.contains(f));
        if let (Some(a), Some(b)) = (a, b)
            && let Ok(pts) = body.sample_edge(e, &crate::kernel::TessellationOptions::default())
        {
            let d = pts
                .iter()
                .map(|p| (p - hint).norm())
                .fold(Real::INFINITY, Real::min);
            shared.push((d, e, a, b));
        }
    }
    shared.sort_by(|x, y| x.0.total_cmp(&y.0));
    let &(_, e, f1, f2) = shared.first().ok_or("the edge no longer exists")?;
    let target = solid_of(body, f1).ok_or("the edge has no solid")?;

    // A planar cap and a cylinder: circular treatment.
    let (cap, side) = match (cylinder_of(body, f1), cylinder_of(body, f2)) {
        (None, Some(c)) => (Some((f1, f2, c)), None::<()>),
        (Some(c), None) => (Some((f2, f1, c)), None),
        _ => (None, None),
    };
    let _ = side;
    if let Some((cap, cyl_face, cyl)) = cap {
        let plane = body.face_plane(cap).ok_or("the cap face is not planar")?;
        let axis = cyl.axis.into_inner();
        let n = plane.normal.into_inner();
        if n.cross(&axis).norm() > 1e-6 {
            return Err("only edges of a cylinder with a perpendicular face".into());
        }
        // Centre of the edge circle: the axis meets the cap plane.
        let o = cyl.origin;
        let t = (plane.origin - o).dot(&n) / axis.dot(&n);
        let center = o + axis * t;
        let edge = body.edge(e).ok_or("edge missing")?;
        let m = body.vertex(edge.start).ok_or("edge missing")?.point;
        // Start the revolution at the circle's own x axis: its facets and
        // the tool's then share vertices (the 96-sector grid).
        let radial = match body.curve(edge.curve) {
            Some(crate::kernel::Curve::Circle(k)) => k.x_axis.into_inner(),
            _ => cyl.x_axis.into_inner(),
        };
        let radial = (radial - axis * radial.dot(&axis))
            .try_normalize(1e-12)
            .or_else(|| ((m - center) - axis * (m - center).dot(&axis)).try_normalize(1e-12))
            .ok_or("degenerate circle")?;
        // Which way the cylinder face runs from the edge, and whether the
        // solid is inside it (boss) or outside (hole) — from the face
        // itself, not from the edge's direction (a closed circle has one
        // vertex).
        let mesh = body
            .face_mesh(cyl_face, &crate::kernel::TessellationOptions::default())
            .map_err(|e| e.to_string())?;
        if mesh.positions.is_empty() {
            return Err("empty cylinder face".into());
        }
        let k = mesh.positions.len() as Real;
        let mean = mesh
            .positions
            .iter()
            .fold(Vector3::zeros(), |acc, p| acc + (p - center))
            / k;
        let side_up = mean.dot(&axis) > 0.0;
        let solid_inside = match body.face(cyl_face) {
            Some(f) if f.facet_of.is_some() => body
                .face_plane(cyl_face)
                .map(|p| {
                    let c = p.origin - center;
                    let rad = c - axis * c.dot(&axis);
                    p.normal.into_inner().dot(&rad) > 0.0
                })
                .unwrap_or(true),
            Some(f) => f.sense == Sense::Same,
            None => true,
        };
        let u2 = if side_up { axis } else { -axis };
        let convex = n.dot(&u2) < 0.0;
        // The cap runs away from the material side of the cylinder wall on
        // a convex edge, towards it on a concave one.
        let m_out = if solid_inside { 1.0 } else { -1.0 }; // wall normal · radial
        let cap_inward = if convex { m_out > 0.0 } else { m_out < 0.0 };
        return Ok(EdgeGeom::Circular {
            center,
            axis,
            radial,
            radius: cyl.radius,
            cap_inward,
            side_up,
            convex,
            target,
        });
    }

    let (p1, p2) = (
        body.face_plane(f1)
            .ok_or("only edges between planar faces")?,
        body.face_plane(f2)
            .ok_or("only edges between planar faces")?,
    );
    let dir = body
        .edge_direction(e)
        .ok_or("only straight edges between planar faces")?
        .normalize();
    // The whole straight run between the two faces: collinear shared edges.
    let mut lo = Real::INFINITY;
    let mut hi = Real::NEG_INFINITY;
    let edge = body.edge(e).ok_or("edge missing")?;
    let base = body.vertex(edge.start).ok_or("edge missing")?.point;
    for &(_, e2, _, _) in &shared {
        let Some(x) = body.edge(e2) else { continue };
        let (Some(a), Some(b)) = (body.vertex(x.start), body.vertex(x.end)) else {
            continue;
        };
        for p in [a.point, b.point] {
            let off = p - base;
            if (off - dir * off.dot(&dir)).norm() < 1e-6 {
                lo = lo.min(off.dot(&dir));
                hi = hi.max(off.dot(&dir));
            }
        }
    }
    let (a, b) = (base + dir * lo, base + dir * hi);
    let (n1, n2) = (p1.normal.into_inner(), p2.normal.into_inner());
    let u1 = n1.cross(&run_dir(body, f1, e).ok_or("edge not on its face")?);
    let u2 = n2.cross(&run_dir(body, f2, e).ok_or("edge not on its face")?);
    let convex = n1.dot(&u2) < 0.0;
    let reach = |f: FaceId, u: &Vector3| {
        body.face_vertices(f)
            .into_iter()
            .filter_map(|v| body.vertex(v))
            .map(|v| (v.point - a).dot(u))
            .fold(0.0, Real::max)
    };
    Ok(EdgeGeom::Straight {
        a,
        b,
        u1,
        u2,
        reach: [reach(f1, &u1), reach(f2, &u2)],
        convex,
        target,
    })
}

/// A test for faces named `name`. When no face has that name any more but
/// faces of the same feature exist (names from an older kernel, e.g.
/// `Fillet1#4:Side:Face` for today's `Fillet1:Round[…]`), any face of that
/// feature matches; the edge's hint point then picks the edge.
pub fn face_matcher<'a>(body: &BRep, name: &'a str) -> impl Fn(Option<&str>) -> bool + 'a {
    let feature = |n: &str| -> String {
        let head = n.split(':').next().unwrap_or(n);
        head.split('#').next().unwrap_or(head).to_owned()
    };
    let exists = body.faces().any(|(_, f)| f.name.as_deref() == Some(name));
    let wanted = feature(name);
    move |n: Option<&str>| match n {
        Some(n) if exists => n == name,
        Some(n) => feature(n) == wanted,
        None => false,
    }
}

/// The reference of edge `e` of `body` (its two faces must be named).
pub fn edge_ref(body: &BRep, e: EdgeId) -> Option<EdgeRef> {
    let fs = body.edge_faces(e);
    let names: Vec<String> = fs
        .iter()
        .filter_map(|f| body.face(*f).and_then(|f| f.name.clone()))
        .collect();
    if names.len() != 2 || names[0] == names[1] {
        return None;
    }
    let x = body.edge(e)?;
    let (a, b) = (body.vertex(x.start)?.point, body.vertex(x.end)?.point);
    // A straight edge: its middle. A curved one: the point half way along
    // the curve (the chord's middle of a half circle is its centre — both
    // halves of a hole's rim had the same hint and resolved to one).
    let m = match body.edge_direction(e) {
        Some(_) => nalgebra::center(&a, &b),
        None => body
            .sample_edge(e, &crate::kernel::TessellationOptions::default())
            .ok()
            .and_then(|pts| curve_middle(&pts))
            .unwrap_or_else(|| nalgebra::center(&a, &b)),
    };
    Some(EdgeRef {
        faces: [names[0].clone(), names[1].clone()],
        hint: [m.x, m.y, m.z],
    })
}

/// The point half way along a polyline (by length).
fn curve_middle(pts: &[Point3]) -> Option<Point3> {
    let total: Real = pts.windows(2).map(|w| (w[1] - w[0]).norm()).sum();
    let mut left = total / 2.0;
    for w in pts.windows(2) {
        let l = (w[1] - w[0]).norm();
        if l >= left && l > 0.0 {
            return Some(w[0] + (w[1] - w[0]) * (left / l));
        }
        left -= l;
    }
    pts.last().copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::part::{
        ExtrudeFeature, Feature, OriginPlane, PartDocument, PlaneRef, ProfileRef, SketchOptions,
        SketchPlacement,
    };
    use crate::sketch::Point2;

    fn volume(body: &BRep) -> Real {
        body.solids()
            .map(|(id, _)| {
                let mut t = Vec::new();
                crate::kernel::csg::volume(&body.solid_polygons(id, &mut t).unwrap())
            })
            .sum()
    }

    fn top_front(_part: &PartDocument) -> EdgeRef {
        EdgeRef {
            faces: ["Box1:Top".into(), "Box1:Front".into()],
            hint: [40.0, 0.0, 20.0],
        }
    }

    #[test]
    fn fillet_and_chamfer_a_box_edge() {
        let base = PartDocument::with_box("Box1", [80.0, 60.0, 20.0]).unwrap();
        let v0 = volume(&base.body);
        // Fillet r = 5 on a convex edge: removes (1 − π/4)·r²·80.
        let mut part = base.clone();
        let id = part
            .add_blend(BlendFeature::fillet(vec![top_front(&part)], 5.0))
            .unwrap();
        assert_eq!(id, "Fillet1");
        let lost = v0 - volume(&part.body);
        let exact = (1.0 - PI / 4.0) * 25.0 * 80.0;
        assert!((lost - exact).abs() / exact < 0.02, "{lost} vs {exact}");
        for (s, _) in part.body.solids() {
            part.body.validate_solid(s).unwrap();
        }
        assert!(
            part.body
                .faces()
                .any(|(_, f)| f.name.as_deref().is_some_and(|n| n.starts_with("Fillet1")))
        );
        // Chamfer 4: removes 4²/2·80; two distances 3 × 6: 9·80.
        let mut part = base.clone();
        part.add_blend(BlendFeature::chamfer(vec![top_front(&part)], 4.0))
            .unwrap();
        let lost = v0 - volume(&part.body);
        assert!((lost - 8.0 * 80.0).abs() < 1.0, "{lost}");
        let mut part = base.clone();
        let mut c = BlendFeature::chamfer(vec![top_front(&part)], 3.0);
        if let BlendKind::Chamfer {
            method, distance2, ..
        } = &mut c.kind
        {
            *method = ChamferMethod::TwoDistances;
            *distance2 = 6.0;
        }
        part.add_blend(c).unwrap();
        let lost = v0 - volume(&part.body);
        assert!((lost - 9.0 * 80.0).abs() < 1.0, "{lost}");
        // Too large a radius fails and changes nothing.
        let mut part = base.clone();
        assert!(
            part.add_blend(BlendFeature::fillet(vec![top_front(&part)], 50.0))
                .is_err()
        );
        assert_eq!(part.features.len(), 1);
    }

    /// A concave edge (block on a plate) gains material; a circular edge of
    /// a cylinder boss on the plate is rounded by a revolved tool.
    #[test]
    fn concave_and_circular_edges() {
        let mut part = PartDocument::with_box("Box1", [80.0, 60.0, 10.0]).unwrap();
        let opts = SketchOptions {
            auto_project_origin: false,
            auto_reference_edges: false,
            ..Default::default()
        };
        let i = part
            .create_sketch(
                SketchPlacement {
                    plane: PlaneRef::Face("Box1:Top".into()),
                    ..Default::default()
                },
                &opts,
            )
            .unwrap();
        let sk = &mut part.sketches[i].sketch;
        sk.add_circle(Point2::new(40.0, 30.0), 10.0).unwrap();
        let r = sk.regions();
        let pr = ProfileRef::of(&r[0], r[0].interior_point());
        let mut ex = ExtrudeFeature::new(vec![pr], 15.0);
        ex.operation = crate::part::Operation::Join;
        let boss = part.add_extrude(i, ex).unwrap();
        let v0 = volume(&part.body);
        // Pick the boss's top circular edge and its base (concave) edge.
        let edges: Vec<EdgeRef> = part
            .body
            .edges()
            .filter_map(|(e, _)| edge_ref(&part.body, e))
            .collect();
        let top = edges
            .iter()
            .find(|r| r.hint[2] > 20.0 && r.faces.iter().any(|f| f.starts_with(&boss)))
            .cloned()
            .expect("top edge of the boss");
        let mut p = part.clone();
        p.add_blend(BlendFeature::fillet(vec![top], 2.0)).unwrap();
        let lost = v0 - volume(&p.body);
        // Torus segment: (1 − π/4)·r²·2π·(R − ...) ≈ 0.2146·4·2π·(10 − 0.78)
        let exact = (1.0 - PI / 4.0)
            * 4.0
            * TAU
            * (10.0 - 2.0 * (4.0 - PI) / (3.0 * (4.0 - PI)) * 2.0 / 2.0);
        assert!(
            lost > 0.0 && (lost - exact).abs() / exact < 0.15,
            "{lost} vs {exact}"
        );
        let base = edges
            .iter()
            .find(|r| {
                (r.hint[2] - 10.0).abs() < 0.5 && r.faces.iter().any(|f| f.starts_with(&boss))
            })
            .cloned()
            .expect("base edge of the boss");
        let mut p = part.clone();
        p.add_blend(BlendFeature::fillet(vec![base], 2.0)).unwrap();
        assert!(volume(&p.body) > v0, "a concave fillet adds material");
        // Save / load / regenerate keeps it.
        let json = serde_json::to_string(&p).unwrap();
        let mut back: PartDocument = serde_json::from_str(&json).unwrap();
        back.regenerate().unwrap();
        assert!(back.feature_health.is_empty(), "{:?}", back.feature_health);
        assert!((volume(&back.body) - volume(&p.body)).abs() < 1e-6);
        let _ = (Feature::Box, OriginPlane::XY);
    }

    /// The owner's test: a 40×30×20 box, fillet r = 5 on the 4 vertical and
    /// the 4 bottom edges (spherical bottom corners), then a chamfer 2 on the
    /// whole top loop, straight parts and fillet arcs alike.
    #[test]
    fn rounded_box_with_chamfered_top() {
        let (a, b, h, r, d) = (40.0, 30.0, 20.0, 5.0, 2.0);
        let mut part = PartDocument::with_box("Box1", [a, b, h]).unwrap();
        let f = |x: &str, y: &str, hint: [Real; 3]| EdgeRef {
            faces: [format!("Box1:{x}"), format!("Box1:{y}")],
            hint,
        };
        let edges = vec![
            f("Front", "Right", [a, 0.0, h / 2.0]),
            f("Right", "Back", [a, b, h / 2.0]),
            f("Back", "Left", [0.0, b, h / 2.0]),
            f("Left", "Front", [0.0, 0.0, h / 2.0]),
            f("Bottom", "Front", [a / 2.0, 0.0, 0.0]),
            f("Bottom", "Right", [a, b / 2.0, 0.0]),
            f("Bottom", "Back", [a / 2.0, b, 0.0]),
            f("Bottom", "Left", [0.0, b / 2.0, 0.0]),
        ];
        part.add_blend(BlendFeature::fillet(edges, r)).unwrap();
        let v_fillet = a * b * h
            - (4.0 - PI) * r * r * h
            - (1.0 - PI / 4.0) * r * r * 2.0 * (a + b - 4.0 * r)
            - 4.0 * PI * r * r * r / 12.0;
        let v = volume(&part.body);
        assert!((v - v_fillet).abs() / v_fillet < 0.005, "{v} vs {v_fillet}");
        for (s, _) in part.body.solids() {
            part.body.validate_solid(s).unwrap();
        }
        // Spherical corners: every vertex near the bottom corner lies on or
        // inside the sphere of radius r around (r, r, r).
        let c = Point3::new(r, r, r);
        for (_, x) in part.body.vertices() {
            let p = x.point;
            if p.x < r && p.y < r && p.z < r {
                assert!(
                    (p - c).norm() <= r + 0.05,
                    "{p:?} outside the corner sphere"
                );
            }
        }
        // The top loop by Automatic Edge Chain from one top edge.
        let top_edge = part
            .body
            .edges()
            .find(|(e, _)| {
                edge_ref(&part.body, *e).is_some_and(|x| {
                    x.faces.contains(&"Box1:Top".to_owned())
                        && x.faces.contains(&"Box1:Front".to_owned())
                })
            })
            .map(|(e, _)| e)
            .unwrap();
        let chain = tangent_chain(&part.body, top_edge);
        assert_eq!(chain.len(), 8, "4 straight parts and 4 arcs: {chain:?}");
        part.add_blend(BlendFeature::chamfer(chain, d)).unwrap();
        let removed = d * d / 2.0 * 2.0 * (a + b - 4.0 * r)
            + 4.0 * (d * d / 2.0) * (PI / 2.0) * (r - d / 3.0);
        let v2 = volume(&part.body);
        let exact = v_fillet - removed;
        assert!((v2 - exact).abs() / exact < 0.005, "{v2} vs {exact}");
        for (s, _) in part.body.solids() {
            part.body.validate_solid(s).unwrap();
        }
    }

    /// The same box in two Fillet features (vertical edges, then the
    /// bottom loop by its chain) and a third, Chamfer, of the top loop.
    #[test]
    fn rounded_box_in_steps() {
        let (a, b, h, r) = (40.0, 30.0, 20.0, 5.0);
        let mut part = PartDocument::with_box("Box1", [a, b, h]).unwrap();
        let f = |x: &str, y: &str, hint: [Real; 3]| EdgeRef {
            faces: [format!("Box1:{x}"), format!("Box1:{y}")],
            hint,
        };
        part.add_blend(BlendFeature::fillet(
            vec![
                f("Front", "Right", [a, 0.0, h / 2.0]),
                f("Right", "Back", [a, b, h / 2.0]),
                f("Back", "Left", [0.0, b, h / 2.0]),
                f("Left", "Front", [0.0, 0.0, h / 2.0]),
            ],
            r,
        ))
        .unwrap();
        let chain_of = |part: &PartDocument, cap: &str| {
            let e = part
                .body
                .edges()
                .find(|(e, _)| {
                    edge_ref(&part.body, *e).is_some_and(|x| {
                        x.faces.contains(&cap.to_owned())
                            && x.faces.contains(&"Box1:Front".to_owned())
                    })
                })
                .map(|(e, _)| e)
                .unwrap();
            tangent_chain(&part.body, e)
        };
        let bottom = chain_of(&part, "Box1:Bottom");
        assert_eq!(bottom.len(), 8);
        part.add_blend(BlendFeature::fillet(bottom, r)).unwrap();
        for (s, _) in part.body.solids() {
            part.body.validate_solid(s).unwrap();
        }
        let top = chain_of(&part, "Box1:Top");
        part.add_blend(BlendFeature::chamfer(top, 2.0)).unwrap();
        for (s, _) in part.body.solids() {
            part.body.validate_solid(s).unwrap();
        }
    }

    /// The GUI case: a Ø12 boss on a 40×25 plate; rounding its top and then
    /// its (concave) base. The base needed the nudged retry (tangent
    /// contact left degenerate slivers).
    #[test]
    fn boss_top_then_base() {
        let mut part = PartDocument::default();
        let opts = SketchOptions::default();
        let i = part
            .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts)
            .unwrap();
        let sk = &mut part.sketches[i].sketch;
        sk.add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(40.0, 25.0))
            .unwrap();
        let r = sk.regions();
        part.add_extrude(
            i,
            ExtrudeFeature::new(vec![ProfileRef::of(&r[0], r[0].interior_point())], 10.0),
        )
        .unwrap();
        let j = part
            .create_sketch(
                SketchPlacement {
                    plane: PlaneRef::Face("Extrusion1:End".into()),
                    ..Default::default()
                },
                &opts,
            )
            .unwrap();
        let sk = &mut part.sketches[j].sketch;
        sk.add_circle(Point2::new(20.0, 12.5), 6.0).unwrap();
        let r = sk.regions();
        let small = r
            .iter()
            .min_by(|a, b| a.area().total_cmp(&b.area()))
            .unwrap();
        let mut ex = ExtrudeFeature::new(vec![ProfileRef::of(small, small.interior_point())], 10.0);
        ex.operation = crate::part::Operation::Join;
        part.add_extrude(j, ex).unwrap();
        let v0 = volume(&part.body);
        let refs: Vec<EdgeRef> = part
            .body
            .edges()
            .filter_map(|(e, _)| edge_ref(&part.body, e))
            .collect();
        let top = refs.iter().find(|r| r.hint[2] > 19.0).unwrap().clone();
        part.add_blend(BlendFeature::fillet(vec![top], 2.0))
            .unwrap();
        let v1 = volume(&part.body);
        assert!(v1 < v0);
        let base = refs
            .iter()
            .find(|r| {
                (r.hint[2] - 10.0).abs() < 0.5
                    && r.faces.iter().any(|f| f.starts_with("Extrusion2"))
            })
            .unwrap()
            .clone();
        part.add_blend(BlendFeature::fillet(vec![base], 2.0))
            .unwrap();
        assert!(volume(&part.body) > v1, "the base fillet adds material");
    }
}

/// The angular range an edge set between a planar cap and a cylinder
/// covers around the axis: (start, sweep) about `axis`, measured from
/// `radial`; a full circle is (0, 2π).
fn arc_range(
    body: &BRep,
    r: &EdgeRef,
    center: &Point3,
    axis: &Vector3,
    radial: &Vector3,
) -> (Real, Real) {
    let fa = faces_named(body, &r.faces[0]);
    let fb = faces_named(body, &r.faces[1]);
    let y = axis.cross(radial);
    let mut angles: Vec<Real> = Vec::new();
    for (e, _) in body.edges() {
        let fs = body.edge_faces(e);
        if !(fs.iter().any(|f| fa.contains(f)) && fs.iter().any(|f| fb.contains(f))) {
            continue;
        }
        if let Ok(pts) = body.sample_edge(e, &crate::kernel::TessellationOptions::default()) {
            for p in pts {
                let v = p - center;
                angles.push(v.dot(&y).atan2(v.dot(radial)).rem_euclid(TAU));
            }
        }
    }
    if angles.len() < 2 {
        return (0.0, TAU);
    }
    angles.sort_by(Real::total_cmp);
    // The largest gap between consecutive angles is the part not covered.
    let mut gap = (angles[0] + TAU - angles[angles.len() - 1], angles.len() - 1);
    for k in 1..angles.len() {
        let g = angles[k] - angles[k - 1];
        if g > gap.0 {
            gap = (g, k - 1);
        }
    }
    if gap.0 < 0.2 {
        return (0.0, TAU);
    }
    let start = angles[(gap.1 + 1) % angles.len()];
    (start, TAU - gap.0)
}

/// The outer loop of a planar face as a profile in its own plane (normal
/// out of the solid): lines, and exact arcs where it borders a cylinder
/// (or its facets) with axis along the normal.
struct CapLoop {
    face: FaceId,
    plane: Plane,
    segments: Vec<ProfileSegment>,
    /// Smallest arc radius (∞ without arcs).
    min_arc: Real,
}

/// What borders the cap along one coedge.
#[derive(Clone, Debug, PartialEq)]
enum Side {
    Wall(String),
    Arc(String, P2, Real),
}

fn cap_loop(body: &BRep, cap: &str) -> Result<CapLoop, String> {
    let face_id = faces_named(body, cap)
        .into_iter()
        .filter(|f| body.face_plane(*f).is_some())
        .max_by_key(|f| body.face(*f).map_or(0, |x| x.outer.coedges.len()))
        .ok_or_else(|| format!("face '{cap}' no longer exists"))?;
    let plane = body.face_plane(face_id).ok_or("the face is not planar")?;
    let n = plane.normal.into_inner();
    let face = body.face(face_id).ok_or("face missing")?;
    let to2 = |p: &Point3| plane.project(p);
    // (side, start, end) per coedge, in loop order.
    let mut runs: Vec<(Side, P2, P2)> = Vec::new();
    for ce in &face.outer.coedges {
        let edge = body.edge(ce.edge).ok_or("edge missing")?;
        let (mut a, mut b) = (
            body.vertex(edge.start).ok_or("vertex missing")?.point,
            body.vertex(edge.end).ok_or("vertex missing")?.point,
        );
        if ce.sense == Sense::Reversed {
            std::mem::swap(&mut a, &mut b);
        }
        let other = body
            .edge_faces(ce.edge)
            .into_iter()
            .find(|&f| f != face_id)
            .ok_or("open edge")?;
        let oname = body
            .face(other)
            .and_then(|f| f.name.clone())
            .unwrap_or_default();
        // The wall must be perpendicular to the cap and below it.
        let side = match cylinder_of(body, other) {
            Some(c) => {
                if c.axis.into_inner().cross(&n).norm() > 1e-6 {
                    return Err("a wall is not perpendicular to the face".into());
                }
                Side::Arc(oname, to2(&c.origin), c.radius)
            }
            None => {
                let wp = body
                    .face_plane(other)
                    .ok_or("only planar or cylindrical walls")?;
                if wp.normal.into_inner().dot(&n).abs() > 1e-6 {
                    return Err("a wall is not perpendicular to the face".into());
                }
                Side::Wall(oname)
            }
        };
        let below = body
            .face_vertices(other)
            .into_iter()
            .filter_map(|v| body.vertex(v))
            .all(|v| (v.point - plane.origin).dot(&n) <= 1e-6);
        if !below {
            return Err("the edge is concave (a wall rises above the face)".into());
        }
        // A curved edge (exact circle): split into its chord ends only.
        let (pa, pb) = (to2(&a), to2(&b));
        if (pb - pa).norm() < 1e-12 {
            // A full circle on its own: the whole loop is one arc.
            if let Side::Arc(_, c, r) = &side {
                let a0 = (pa - c).y.atan2((pa - c).x);
                return Ok(CapLoop {
                    face: face_id,
                    plane,
                    segments: vec![ProfileSegment {
                        geometry: Segment2::Arc {
                            c: *c,
                            r: *r,
                            a0,
                            sweep: TAU,
                        },
                        name: "A0".into(),
                    }],
                    min_arc: *r,
                });
            }
            continue;
        }
        runs.push((side, pa, pb));
    }
    if runs.is_empty() {
        return Err("empty loop".into());
    }
    // Start at the beginning of a run.
    let m = runs.len();
    let start = (0..m)
        .find(|&k| runs[k].0 != runs[(k + m - 1) % m].0)
        .unwrap_or(0);
    runs.rotate_left(start);
    let mut segments = Vec::new();
    let mut min_arc = Real::INFINITY;
    let mut k = 0;
    while k < m {
        let side = runs[k].0.clone();
        let mut j = k;
        let mut sweep = 0.0;
        while j < m && runs[j].0 == side {
            if let Side::Arc(_, c, _) = &side {
                let (p, q) = (runs[j].1 - c, runs[j].2 - c);
                sweep += p.perp(&q).atan2(p.dot(&q));
            }
            j += 1;
        }
        let (p0, p1) = (runs[k].1, runs[j - 1].2);
        let geometry = match &side {
            Side::Wall(_) => Segment2::Line { a: p0, b: p1 },
            Side::Arc(_, c, r) => {
                min_arc = min_arc.min(*r);
                Segment2::Arc {
                    c: *c,
                    r: *r,
                    a0: (p0 - c).y.atan2((p0 - c).x),
                    sweep,
                }
            }
        };
        segments.push(ProfileSegment {
            geometry,
            name: format!("S{}", segments.len()),
        });
        k = j;
    }
    // Lines of one wall split by facets of the same plane merge above;
    // collinear neighbours of different walls stay apart (their own faces).
    Ok(CapLoop {
        face: face_id,
        plane,
        segments,
        min_arc,
    })
}

/// Splits the edge set into whole outer loops of planar faces that can be
/// treated as a loop (by face name), and the remaining single edges.
fn split_loops(body: &BRep, refs: &[EdgeRef]) -> (Vec<String>, Vec<EdgeRef>) {
    let has = |a: &str, b: &str| {
        refs.iter()
            .any(|r| (r.faces[0] == a && r.faces[1] == b) || (r.faces[0] == b && r.faces[1] == a))
    };
    let mut caps: Vec<String> = Vec::new();
    let mut names: Vec<String> = refs.iter().flat_map(|r| r.faces.clone()).collect();
    names.sort();
    names.dedup();
    for cap in names {
        let Some(face) = faces_named(body, &cap)
            .into_iter()
            .find(|f| body.face_plane(*f).is_some())
        else {
            continue;
        };
        let Some(f) = body.face(face) else { continue };
        if f.outer.coedges.len() < 2 {
            continue;
        }
        let all = f.outer.coedges.iter().all(|ce| {
            body.edge_faces(ce.edge)
                .into_iter()
                .find(|&o| o != face)
                .and_then(|o| body.face(o).and_then(|x| x.name.clone()))
                .is_some_and(|o| has(&cap, &o))
        });
        // Caps whose loops share no edge with an accepted one.
        let disjoint = caps.iter().all(|c| {
            !refs
                .iter()
                .any(|r| r.faces.contains(c) && r.faces.contains(&cap))
        });
        if all && disjoint && cap_loop(body, &cap).is_ok() {
            caps.push(cap);
        }
    }
    let singles = refs
        .iter()
        .filter(|r| !caps.iter().any(|c| r.faces.contains(c)))
        .cloned()
        .collect();
    (caps, singles)
}

/// Automatic Edge Chain: the edge `e` and every edge tangentially connected
/// to it (one reference per pair of faces), as Inventor selects by default.
pub fn tangent_chain(body: &BRep, e: EdgeId) -> Vec<EdgeRef> {
    let opts = crate::kernel::TessellationOptions::default();
    let names = |e: EdgeId| -> Option<(String, String)> {
        let mut v: Vec<String> = body
            .edge_faces(e)
            .iter()
            .filter_map(|f| body.face(*f).and_then(|x| x.name.clone()))
            .collect();
        if v.len() != 2 || v[0] == v[1] {
            return None;
        }
        v.sort();
        Some((v[0].clone(), v[1].clone()))
    };
    // Edges per face pair, and per vertex.
    let mut by_pair: std::collections::HashMap<(String, String), Vec<EdgeId>> = Default::default();
    let mut at_vertex: std::collections::HashMap<crate::kernel::VertexId, Vec<EdgeId>> =
        Default::default();
    for (id, x) in body.edges() {
        if let Some(p) = names(id) {
            by_pair.entry(p).or_default().push(id);
            at_vertex.entry(x.start).or_default().push(id);
            at_vertex.entry(x.end).or_default().push(id);
        }
    }
    // Direction of edge `id` leaving vertex `v`.
    let leave = |id: EdgeId, v: crate::kernel::VertexId| -> Option<Vector3> {
        let x = body.edge(id)?;
        let pts = body.sample_edge(id, &opts).ok()?;
        if pts.len() < 2 {
            return None;
        }
        let d = if x.start == v {
            pts[1] - pts[0]
        } else {
            pts[pts.len() - 2] - pts[pts.len() - 1]
        };
        d.try_normalize(1e-12)
    };
    let Some(first) = names(e) else {
        return Vec::new();
    };
    let mut done: Vec<(String, String)> = vec![first.clone()];
    let mut queue = vec![first];
    while let Some(pair) = queue.pop() {
        let edges = by_pair.get(&pair).cloned().unwrap_or_default();
        for &id in &edges {
            let Some(x) = body.edge(id) else { continue };
            for v in [x.start, x.end] {
                let Some(out) = leave(id, v) else { continue };
                for &o in at_vertex.get(&v).map(Vec::as_slice).unwrap_or(&[]) {
                    let Some(op) = names(o) else { continue };
                    if done.contains(&op) {
                        continue;
                    }
                    // Must share a face and continue smoothly.
                    let shares = [&op.0, &op.1]
                        .iter()
                        .any(|n| **n == pair.0 || **n == pair.1);
                    if !shares {
                        continue;
                    }
                    if let Some(d) = leave(o, v)
                        && d.dot(&-out) > (12.0 as Real).to_radians().cos()
                    {
                        done.push(op.clone());
                        queue.push(op);
                    }
                }
            }
        }
    }
    done.into_iter()
        .filter_map(|pair| {
            let edges = by_pair.get(&pair)?;
            let id = edges[edges.len() / 2];
            let x = body.edge(id)?;
            let (a, b) = (body.vertex(x.start)?.point, body.vertex(x.end)?.point);
            let m = nalgebra::center(&a, &b);
            Some(EdgeRef {
                faces: [pair.0, pair.1],
                hint: [m.x, m.y, m.z],
            })
        })
        .collect()
}
