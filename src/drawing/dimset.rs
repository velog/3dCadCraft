//! Baseline, chain and ordinate dimensions (`docs/spec/4.7-drawing-commands.md`):
//! points of one view measured along one direction from an origin (baseline,
//! ordinate) or end to end (chain). A set is one annotation (format revision
//! 29); Baseline and Chain without "Set" become separate dimensions when
//! created. Drawing is in `annot::Draw::dim_set`.

use serde::{Deserialize, Serialize};

use super::annot::PointRef;

/// How the points are dimensioned.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SetKind {
    /// Each point from the origin, the dimensions stacked by the style's
    /// spacing.
    Baseline,
    /// Neighbouring points end to end on one line.
    Chain,
    /// The distance from the origin written at the end of each point's
    /// extension line.
    Ordinate,
}

impl SetKind {
    /// The command's name.
    pub fn label(self) -> &'static str {
        match self {
            SetKind::Baseline => "Baseline",
            SetKind::Chain => "Chain",
            SetKind::Ordinate => "Ordinate",
        }
    }
}

/// The measured direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SetDir {
    /// Along the sheet's x.
    Horizontal,
    /// Along the sheet's y.
    Vertical,
}

impl SetDir {
    /// The measured axis (0 = x).
    pub fn axis(self) -> usize {
        match self {
            SetDir::Horizontal => 0,
            SetDir::Vertical => 1,
        }
    }
}

/// A baseline / chain / ordinate set.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DimSet {
    /// Kind (saved as `set`: `kind` is the annotation's tag).
    #[serde(rename = "set")]
    pub kind: SetKind,
    /// Direction.
    pub dir: SetDir,
    /// The points (one view).
    pub points: Vec<PointRef>,
    /// Which point is the origin (baseline, ordinate).
    #[serde(default)]
    pub origin: usize,
    /// Where it was placed, relative to the view's position (sheet mm): the
    /// nearest dimension line / the ordinate texts pass through it.
    pub at: [f64; 2],
}

impl DimSet {
    /// The view.
    pub fn view(&self) -> Option<u32> {
        self.points.first().map(|p| p.view)
    }

    /// The direction a placement point `at` (sheet) gives for points at
    /// `pts` (sheet): beside their box → vertical, else horizontal.
    pub fn dir_for(pts: &[[f64; 2]], at: [f64; 2]) -> SetDir {
        let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
        for p in pts {
            for k in 0..2 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        let beside = at[0] < lo[0] || at[0] > hi[0];
        let above = at[1] < lo[1] || at[1] > hi[1];
        if beside && !above {
            SetDir::Vertical
        } else {
            SetDir::Horizontal
        }
    }

    /// The order the points are drawn in (indices into `points`) from their
    /// coordinates `c` along the direction: points at one coordinate count
    /// once (the first). Baseline: the origin, then by distance from it;
    /// chain / ordinate: by coordinate.
    pub fn order(&self, c: &[Option<f64>]) -> Vec<usize> {
        let mut idx: Vec<usize> = (0..c.len()).filter(|&k| c[k].is_some()).collect();
        let val = |k: usize| c[k].unwrap_or_default();
        let o = c.get(self.origin).copied().flatten();
        match (self.kind, o) {
            (SetKind::Baseline, Some(o)) => {
                idx.sort_by(|&a, &b| {
                    let (da, db) = ((val(a) - o).abs(), (val(b) - o).abs());
                    da.total_cmp(&db)
                        .then((a != self.origin).cmp(&(b != self.origin)))
                });
            }
            _ => idx.sort_by(|&a, &b| val(a).total_cmp(&val(b))),
        }
        let mut out: Vec<usize> = Vec::new();
        for k in idx {
            // The origin wins its coordinate.
            if let Some(j) = out.iter().position(|&j| (val(j) - val(k)).abs() < 1e-6) {
                if k == self.origin {
                    out[j] = k;
                }
                continue;
            }
            out.push(k);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drawing::annot::PointRole;

    fn set(kind: SetKind, n: usize, origin: usize) -> DimSet {
        DimSet {
            kind,
            dir: SetDir::Horizontal,
            points: (0..n)
                .map(|k| PointRef {
                    view: 1,
                    edge: format!("E{k}"),
                    role: PointRole::Start,
                })
                .collect(),
            origin,
            at: [0.0, 0.0],
        }
    }

    #[test]
    fn baseline_order_from_the_origin() {
        let c = [
            Some(30.0),
            Some(0.0),
            Some(10.0),
            Some(0.0),
            None,
            Some(50.0),
        ];
        // Origin at 30: 30, then 10 / 50 (20 each), then 0 once.
        assert_eq!(set(SetKind::Baseline, 6, 0).order(&c), [0, 2, 5, 1]);
        // Chain: by coordinate, duplicates once.
        assert_eq!(set(SetKind::Chain, 6, 0).order(&c), [1, 2, 0, 5]);
        // The origin keeps its place when another point shares it.
        assert_eq!(set(SetKind::Ordinate, 6, 3).order(&c), [3, 2, 0, 5]);
    }

    #[test]
    fn direction_from_the_placement() {
        let pts = [[0.0, 0.0], [50.0, 20.0]];
        assert_eq!(DimSet::dir_for(&pts, [25.0, 40.0]), SetDir::Horizontal);
        assert_eq!(DimSet::dir_for(&pts, [25.0, -10.0]), SetDir::Horizontal);
        assert_eq!(DimSet::dir_for(&pts, [70.0, 10.0]), SetDir::Vertical);
        assert_eq!(DimSet::dir_for(&pts, [-5.0, 10.0]), SetDir::Vertical);
    }
}
