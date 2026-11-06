//! Sketches of drawing views (`docs/spec/4-drawings.md`, stage D-S5 / D-S6):
//! a [`Sketch`] in a view's plane (view-plane mm, unscaled, as
//! `ViewFrame::project`), edited with the part sketch's tools. A section
//! line is such a sketch of its parent view: its lines, end to end, are the
//! line's points (`ViewKind::Section`), which the section is computed from.
//!
//! The user's own drawing sketches (spec 4.12) are [`DrawingSketch`]es: on a
//! sheet (sheet mm) or owned by a view (its view plane, model mm, so they
//! move and scale with it).

use serde::{Deserialize, Serialize};

use crate::kernel::Plane;
use crate::sketch::{Entity, Point2, PointId, Sketch, Style};

/// A sketch the user made on a drawing (New Sketch, Draft View): on a sheet
/// or owned by a view.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DrawingSketch {
    /// Identity inside the drawing (never reused).
    pub id: u32,
    /// Browser name (`Sketch1`).
    pub name: String,
    /// Drawn on the sheet (Visibility).
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub visible: bool,
    /// The geometry: sheet mm for a sheet sketch, the view plane (model mm)
    /// for a view's.
    pub sketch: ViewSketch,
}

fn yes() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

/// How a curve of a finished drawing sketch is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurveLook {
    /// A continuous line.
    Solid,
    /// A centreline: a chain line.
    Chain,
}

impl DrawingSketch {
    /// What the sketch shows once finished: its normal curves and
    /// centrelines as polylines (its coordinates); construction geometry,
    /// points, constraints and dimensions show only while it is edited
    /// (Inventor's Sketch Only geometry and sizing dimensions).
    pub fn curves(&self) -> Vec<(Vec<[f64; 2]>, CurveLook)> {
        let sk = &self.sketch.0;
        let mut out = Vec::new();
        for (id, e) in sk.entities() {
            let look = match sk.style(id) {
                Style::Normal => CurveLook::Solid,
                Style::Centerline => CurveLook::Chain,
                _ => continue,
            };
            let pts = entity_points(sk, id, e);
            if pts.len() >= 2 {
                out.push((pts, look));
            }
        }
        for t in sk.texts() {
            if let Some((o, d)) = sk.text_frame(t) {
                for line in t.polylines_at(o, d) {
                    let pts: Vec<[f64; 2]> = line.iter().map(|p| [p.x, p.y]).collect();
                    if pts.len() >= 2 {
                        out.push((pts, CurveLook::Solid));
                    }
                }
            }
        }
        out
    }

    /// The box of what [`Self::curves`] draws (`None`: nothing).
    pub fn extent(&self) -> Option<([f64; 2], [f64; 2])> {
        let mut lo = [f64::INFINITY; 2];
        let mut hi = [f64::NEG_INFINITY; 2];
        for (pts, _) in self.curves() {
            for p in pts {
                for k in 0..2 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
        }
        (lo[0] <= hi[0]).then_some((lo, hi))
    }
}

/// The points of curve `id` (arcs and circles every 3.75°, splines in 96
/// steps).
fn entity_points(sk: &Sketch, id: crate::sketch::EntityId, e: &Entity) -> Vec<[f64; 2]> {
    use std::f64::consts::TAU;
    let at = |p: PointId| sk.point(p).map(|q| [q.position.x, q.position.y]);
    let circle = |c: [f64; 2], r: f64, a0: f64, sweep: f64| {
        let n = ((sweep / TAU) * 96.0).ceil().max(2.0) as usize;
        (0..=n)
            .map(|i| {
                let a = a0 + sweep * i as f64 / n as f64;
                [c[0] + r * a.cos(), c[1] + r * a.sin()]
            })
            .collect::<Vec<_>>()
    };
    match e {
        Entity::Line { start, end } => [at(*start), at(*end)].into_iter().flatten().collect(),
        Entity::Circle { center, radius } => at(*center)
            .map(|c| circle(c, *radius, 0.0, TAU))
            .unwrap_or_default(),
        Entity::Arc { center, start, end } => {
            let (Some(c), Some(s), Some(t)) = (at(*center), at(*start), at(*end)) else {
                return Vec::new();
            };
            let a0 = (s[1] - c[1]).atan2(s[0] - c[0]);
            let a1 = (t[1] - c[1]).atan2(t[0] - c[0]);
            let mut sweep = a1 - a0;
            if sweep <= 0.0 {
                sweep += TAU;
            }
            circle(c, (s[0] - c[0]).hypot(s[1] - c[1]), a0, sweep)
        }
        Entity::Spline { .. } => sk.spline_curve(id).map_or_else(Vec::new, |c| {
            (0..=96)
                .map(|i| {
                    let p = c.point_at(i as f64 / 96.0);
                    [p.x, p.y]
                })
                .collect()
        }),
    }
}

/// A sketch of a view (its plane is the view plane: x right, y up).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ViewSketch(pub Sketch);

impl PartialEq for ViewSketch {
    fn eq(&self, other: &Self) -> bool {
        // A sketch has no equality of its own; its saved form is its value.
        serde_json::to_string(&self.0).ok() == serde_json::to_string(&other.0).ok()
    }
}

impl Default for ViewSketch {
    fn default() -> Self {
        Self(Sketch::new(Plane::xy()))
    }
}

/// Why a sketch is no section line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LineError {
    /// No (non-construction) line in it.
    Empty,
    /// Its lines do not form one open chain (several pieces, a closed loop,
    /// a curve that is no line).
    NotOneChain,
}

impl std::fmt::Display for LineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Empty => "the sketch has no line",
            Self::NotOneChain => "the sketch's lines must form one open chain",
        })
    }
}

impl ViewSketch {
    /// A sketch holding the section line `pts`: lines end to end, sharing
    /// their points.
    pub fn from_line(pts: &[[f64; 2]]) -> Self {
        let mut s = Self::default();
        let ids: Vec<_> = pts
            .iter()
            .map(|p| s.0.add_point(Point2::new(p[0], p[1])))
            .collect();
        for w in ids.windows(2) {
            let _ = s.0.add_line_between(w[0], w[1]);
        }
        s
    }

    /// The section line this sketch draws: the points of its normal-style
    /// curves, end to end (construction lines and centrelines are left
    /// out). Every such curve must be a line of one open chain.
    pub fn line(&self) -> Result<Vec<[f64; 2]>, LineError> {
        let ids = self.chain_points()?;
        ids.iter()
            .map(|p| {
                let q = self.0.point(*p).ok_or(LineError::NotOneChain)?.position;
                Ok([q.x, q.y])
            })
            .collect()
    }

    /// The points of the section line's chain, first to last: its
    /// normal-style lines walked end to end through their shared points
    /// (construction geometry touching them is no part of it).
    fn chain_points(&self) -> Result<Vec<PointId>, LineError> {
        let sk = &self.0;
        let mut lines = Vec::new();
        for (id, e) in sk.entities() {
            if sk.style(id) != Style::Normal {
                continue;
            }
            if !e.is_line() {
                return Err(LineError::NotOneChain);
            }
            let (a, b) = sk.line_points(id).map_err(|_| LineError::NotOneChain)?;
            lines.push((a, b));
        }
        if lines.is_empty() {
            return Err(LineError::Empty);
        }
        let degree = |p: PointId| lines.iter().filter(|(a, b)| *a == p || *b == p).count();
        if lines.iter().any(|(a, b)| degree(*a) > 2 || degree(*b) > 2) {
            return Err(LineError::NotOneChain);
        }
        // Start at an end (a point of one line only); none: a closed loop.
        let start = lines
            .iter()
            .flat_map(|(a, b)| [*a, *b])
            .find(|p| degree(*p) == 1)
            .ok_or(LineError::NotOneChain)?;
        let mut used = vec![false; lines.len()];
        let mut pts = vec![start];
        let mut at = start;
        while let Some(k) =
            (0..lines.len()).find(|k| !used[*k] && (lines[*k].0 == at || lines[*k].1 == at))
        {
            used[k] = true;
            at = if lines[k].0 == at {
                lines[k].1
            } else {
                lines[k].0
            };
            pts.push(at);
        }
        if used.iter().any(|u| !u) {
            return Err(LineError::NotOneChain);
        }
        // The direction of the first line drawn (the line's a → b, which
        // the view's side is taken against).
        let (a, b) = lines[0];
        let pos = |p: PointId| pts.iter().position(|q| *q == p);
        if pos(a) > pos(b) {
            pts.reverse();
        }
        Ok(pts)
    }

    /// The sketch follows a section line moved without it (grips, points
    /// tied to the model): when its chain has as many points, they move
    /// there; else the sketch is made again from the line.
    pub fn follow(&mut self, pts: &[[f64; 2]]) {
        let same = self.line().is_ok_and(|l| {
            l.len() == pts.len()
                && l.iter()
                    .zip(pts)
                    .all(|(a, b)| (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-9)
        });
        if same {
            return;
        }
        if self.line().is_ok_and(|l| l.len() == pts.len()) && self.move_chain(pts) {
            return;
        }
        *self = Self::from_line(pts);
    }

    /// Moves the chain's points onto `pts` (same count) and solves; false
    /// when the constraints do not allow it (the sketch is left as it was).
    fn move_chain(&mut self, pts: &[[f64; 2]]) -> bool {
        let before = self.0.clone();
        let Ok(ids) = self.chain_points() else {
            return false;
        };
        let sk = &mut self.0;
        for (id, p) in ids.iter().zip(pts) {
            let _ = sk.move_point(*id, Point2::new(p[0], p[1]));
        }
        let ok = matches!(sk.solve(), Ok(r) if r.status == crate::sketch::SolveStatus::Solved)
            && self.line().is_ok_and(|l| {
                l.iter()
                    .zip(pts)
                    .all(|(a, b)| (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-6)
            });
        if !ok {
            self.0 = before;
        }
        ok
    }
}
