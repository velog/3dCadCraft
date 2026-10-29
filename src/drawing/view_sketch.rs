//! Sketches of drawing views (`docs/spec/4-drawings.md`, stage D-S5 / D-S6):
//! a [`Sketch`] in a view's plane (view-plane mm, unscaled, as
//! `ViewFrame::project`), edited with the part sketch's tools. A section
//! line is such a sketch of its parent view: its lines, end to end, are the
//! line's points (`ViewKind::Section`), which the section is computed from.

use serde::{Deserialize, Serialize};

use crate::kernel::Plane;
use crate::sketch::{Point2, PointId, Sketch, Style};

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
