//! A library fastener's body (`BodyOp::Axial`, format revision 61): a stepped
//! body about Z as its meridian's runs, built face by face in one kernel call.
//! The general Boolean of a hexagon and a shank took ~240 ms a part, and a
//! hexagon's corner chamfers 1.3–2 s (spec 6.3 §6.0b).

use serde::{Deserialize, Serialize};

pub use crate::kernel::exact::{AxialCone, AxialHole, AxialRun, AxialSeg, AxialShape};

/// The runs of the meridian, in order. Faces are named `<feature id>:<role>`
/// with the roles given here, so they stay the same at every size (Change Size).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AxialOp {
    /// The runs.
    pub runs: Vec<AxialRun>,
    /// Role of the disc closing the first run's start (none at a tip, or when
    /// `closed`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_cap: Option<String>,
    /// The last run's planar face joins back to the first run (a nut's bore).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub closed: bool,
    /// Round holes between its planar faces (a flange's bolt holes).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub holes: Vec<AxialHole>,
    /// A straight tee of pipe instead of the runs: an old build finds no runs and
    /// reports the feature, it never builds something else.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tee: Option<AxialTee>,
    /// A bend of pipe instead of the runs, likewise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bend: Option<AxialBend>,
    /// Loose balls and rollers, each a body of its own beside what the runs build (a bearing's
    /// rolling elements at the Full level).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rolling: Vec<RollingElement>,
}

/// A ball, or a roller along Z: a cylinder, a cone or a barrel, maybe leaning.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RollingElement {
    /// A ball's centre, or the middle of a roller's lower end.
    pub at: [f64; 3],
    /// Its radius.
    pub radius: f64,
    /// A roller's length; 0: a ball.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub length: f64,
    /// A cone's radius at its far end; 0: as `radius`.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub end_radius: f64,
    /// How much a barrel's middle stands out over `radius`, which is then the radius of both ends.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub crown: f64,
    /// Radians the roller's far end leans away from Z, outwards from the axis (negative: towards it).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub lean: f64,
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

/// A bend of pipe.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AxialBend {
    /// Outside diameter.
    pub outside: f64,
    /// Wall.
    pub wall: f64,
    /// Bend radius.
    pub radius: f64,
    /// Angle in radians.
    pub angle: f64,
}

/// A straight tee of pipe.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AxialTee {
    /// Outside diameter.
    pub outside: f64,
    /// Wall.
    pub wall: f64,
    /// Centre to end.
    pub centre: f64,
}

impl AxialOp {
    /// Refuses a chain whose rings cannot meet in a planar face: two runs that
    /// do not end at one height, a circle between a hexagon's flats and its
    /// corners, a cone past the flats.
    pub fn validate(&self) -> Result<(), String> {
        if let Some(t) = &self.tee {
            let r = t.outside / 2.0;
            return if t.wall > 0.0 && t.wall < r && r < t.centre {
                Ok(())
            } else {
                Err("a tee needs a wall inside its radius and ends beyond it".into())
            };
        }
        if let Some(b) = &self.bend {
            let r = b.outside / 2.0;
            return if b.wall > 0.0 && b.wall < r && r < b.radius && b.angle > 0.0 {
                Ok(())
            } else {
                Err("a bend needs a wall inside its radius and a radius beyond it".into())
            };
        }
        if self.runs.is_empty() {
            return if self.rolling.is_empty() {
                Err("the part has no shape".into())
            } else {
                Ok(())
            };
        }
        let n = self.runs.len();
        for (k, r) in self.runs.iter().enumerate() {
            if let AxialShape::Hexagon {
                across,
                start_cone,
                end_cone,
                ..
            } = &r.shape
            {
                if *across <= 0.0 {
                    return Err("a hexagon needs a positive size across flats".into());
                }
                for c in [start_cone, end_cone].into_iter().flatten() {
                    if c.radius <= 0.0 || c.radius >= across / 2.0 {
                        return Err("a hexagon's chamfer circle must lie inside its flats".into());
                    }
                }
            }
            let next = if k + 1 < n {
                self.runs.get(k + 1)
            } else if self.closed {
                self.runs.first()
            } else {
                None
            };
            if let (Some((z, a)), Some((w, b))) =
                (r.end_ring(), next.and_then(AxialRun::start_ring))
            {
                if (z - w).abs() > 1e-9 {
                    return Err("two runs do not meet at one height".into());
                }
                if a.crosses(&b) {
                    return Err("a circle crosses a hexagon's corners".into());
                }
            }
        }
        Ok(())
    }
}

/// A run's ring, for the checks above.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Ring {
    Round(f64),
    Flats(f64, f64),
}

impl Ring {
    fn crosses(&self, o: &Ring) -> bool {
        match (*self, *o) {
            (Ring::Round(r), Ring::Flats(s, c)) | (Ring::Flats(s, c), Ring::Round(r)) => {
                r >= s / 2.0 && r <= c
            }
            (Ring::Round(a), Ring::Round(b)) => (a - b).abs() < 1e-9,
            (Ring::Flats(a, ca), Ring::Flats(b, cb)) => {
                // Nested only when one's corners stay inside the other's flats.
                let (corners, flats) = if a < b { (ca, b) } else { (cb, a) };
                corners >= flats / 2.0
            }
        }
    }
}

/// A polygon's ring: across flats, and the radius of its corners.
fn flats(across: f64, sides: u32) -> Ring {
    let half = std::f64::consts::PI / f64::from(sides.max(3));
    Ring::Flats(across, across / 2.0 / half.cos())
}

impl AxialRun {
    fn start_ring(&self) -> Option<(f64, Ring)> {
        match &self.shape {
            AxialShape::Turned { from, .. } => {
                (from[0] > 1e-9).then_some((from[1], Ring::Round(from[0])))
            }
            AxialShape::Hexagon {
                across,
                z0,
                sides,
                start_cone,
                ..
            } => Some((
                *z0,
                start_cone
                    .as_ref()
                    .map_or(flats(*across, *sides), |c| Ring::Round(c.radius)),
            )),
        }
    }

    fn end_ring(&self) -> Option<(f64, Ring)> {
        match &self.shape {
            AxialShape::Turned { segs, .. } => {
                let to = segs.last()?.to;
                (to[0] > 1e-9).then_some((to[1], Ring::Round(to[0])))
            }
            AxialShape::Hexagon {
                across,
                z1,
                sides,
                end_cone,
                ..
            } => Some((
                *z1,
                end_cone
                    .as_ref()
                    .map_or(flats(*across, *sides), |c| Ring::Round(c.radius)),
            )),
        }
    }
}

/// Builders for the runs, so templates read as their meridian.
impl AxialRun {
    /// A chain of straight segments through `points`, each `(radius, z, role)`
    /// giving where the segment ends; the first point's role is unused.
    pub fn turned(points: &[(f64, f64, &str)], after: Option<&str>) -> AxialRun {
        let from = points.first().map_or([0.0, 0.0], |p| [p.0, p.1]);
        AxialRun {
            shape: AxialShape::Turned {
                from,
                segs: points
                    .iter()
                    .skip(1)
                    .map(|p| AxialSeg {
                        to: [p.0, p.1],
                        arc: None,
                        role: p.2.to_owned(),
                    })
                    .collect(),
            },
            after: after.map(str::to_owned),
        }
    }

    /// A plain hexagon run.
    pub fn hexagon(across: f64, z0: f64, z1: f64, role: &str, after: Option<&str>) -> AxialRun {
        AxialRun {
            shape: AxialShape::Hexagon {
                across,
                z0,
                z1,
                role: role.to_owned(),
                sides: 6,
                start_cone: None,
                end_cone: None,
            },
            after: after.map(str::to_owned),
        }
    }
}
