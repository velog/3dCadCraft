//! Sketches of drawing views (`docs/spec/4-drawings.md`, stage D-S5 / D-S6):
//! a [`Sketch`] in a view's plane (view-plane mm, unscaled, as
//! `ViewFrame::project`), edited with the part sketch's tools. A section
//! line is such a sketch of its parent view: its lines, end to end, are the
//! line's points (`ViewKind::Section`), which the section is computed from.

use serde::{Deserialize, Serialize};

use crate::kernel::Plane;
use crate::sketch::{Point2, Sketch, Style};

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
        let sk = &self.0;
        let normal: Vec<_> = sk
            .entities()
            .filter(|(id, _)| sk.style(*id) == Style::Normal)
            .map(|(id, e)| (id, e.is_line()))
            .collect();
        let Some(&(first, _)) = normal.first() else {
            return Err(LineError::Empty);
        };
        if normal.iter().any(|(_, line)| !line) {
            return Err(LineError::NotOneChain);
        }
        let (chain, closed) = sk.chain(first);
        if closed || chain.len() != normal.len() {
            return Err(LineError::NotOneChain);
        }
        let mut pts = Vec::with_capacity(chain.len() + 1);
        for (k, (e, reversed)) in chain.iter().enumerate() {
            let (a, b) = sk.line_geometry(*e).ok_or(LineError::NotOneChain)?;
            let (a, b) = if *reversed { (b, a) } else { (a, b) };
            if k == 0 {
                pts.push([a.x, a.y]);
            }
            pts.push([b.x, b.y]);
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
        let sk = &mut self.0;
        let Some(first) = sk
            .entities()
            .find(|(id, _)| sk.style(*id) == Style::Normal)
            .map(|(id, _)| id)
        else {
            return false;
        };
        let (chain, _) = sk.chain(first);
        let mut ids = Vec::new();
        for (k, (e, reversed)) in chain.iter().enumerate() {
            let Ok((a, b)) = sk.line_points(*e) else {
                return false;
            };
            let (a, b) = if *reversed { (b, a) } else { (a, b) };
            if k == 0 {
                ids.push(a);
            }
            ids.push(b);
        }
        for (id, p) in ids.iter().zip(pts) {
            let _ = sk.move_point(*id, Point2::new(p[0], p[1]));
        }
        let ok = sk.solve().is_ok()
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
