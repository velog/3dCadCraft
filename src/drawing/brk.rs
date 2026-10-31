//! Break views (spec 4.10): a stretch of a long view is left out and the
//! pieces on both sides are drawn a gap apart, with break lines at the cut.
//!
//! A break is a 2D operation on the drawn view: the computed lines stay in
//! the view plane's true coordinates (so dimensions measure true values),
//! the removed stretches are clipped out of them ([`apply`]), and the
//! view's [`Placement`](super::annot::Placement) maps points through the
//! [`BreakMap`], which pulls the pieces together.

use serde::{Deserialize, Serialize};

use super::hlr::ViewLines;

/// Inventor's default gap between the pieces (sheet mm).
pub const DEFAULT_GAP: f64 = 6.0;
/// The Display slider's range (Inventor: 1–10, default 5).
pub const DISPLAY_RANGE: std::ops::RangeInclusive<u8> = 1..=10;
/// Breaks one view can hold.
pub const MAX_BREAKS: usize = 8;

/// How the break lines look.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BreakStyle {
    /// Wavy break edges along the cut geometry.
    #[default]
    Rectangular,
    /// A straight line across the view with break ("Z") symbols.
    Structural,
}

/// The direction the view is shortened in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BreakOrientation {
    /// Shortened along the view's x (the break lines are vertical).
    #[default]
    Horizontal,
    /// Shortened along the view's y (the break lines are horizontal).
    Vertical,
}

/// One break of a view.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ViewBreak {
    /// Rectangular or Structural.
    #[serde(default)]
    pub style: BreakStyle,
    /// The direction the view is shortened in (fixed after creation).
    #[serde(default)]
    pub orientation: BreakOrientation,
    /// Start of the removed stretch along the break's axis (view plane,
    /// model mm; `from < to`).
    pub from: f64,
    /// Its end.
    pub to: f64,
    /// The gap between the pieces (sheet mm).
    #[serde(default = "default_gap")]
    pub gap: f64,
    /// Display (1–10): Rectangular — how many waves the break edges have;
    /// Structural — how high the break symbols are.
    #[serde(default = "default_display")]
    pub display: u8,
    /// Break symbols on each break line (Structural, 1–3).
    #[serde(default = "default_symbols")]
    pub symbols: u8,
    /// Suppressed: kept, but the view is drawn as if it were not there
    /// (old builds draw it).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub suppressed: bool,
}

fn default_gap() -> f64 {
    DEFAULT_GAP
}

fn default_display() -> u8 {
    5
}

fn default_symbols() -> u8 {
    1
}

impl ViewBreak {
    /// A break with Inventor's defaults removing `from..to` (taken in
    /// either order) along `orientation`.
    pub fn new(orientation: BreakOrientation, from: f64, to: f64) -> Self {
        Self {
            style: BreakStyle::Rectangular,
            orientation,
            from: from.min(to),
            to: from.max(to),
            gap: DEFAULT_GAP,
            display: default_display(),
            symbols: default_symbols(),
            suppressed: false,
        }
    }

    /// The view-plane axis it shortens (0 = x, 1 = y).
    pub fn axis(&self) -> usize {
        match self.orientation {
            BreakOrientation::Horizontal => 0,
            BreakOrientation::Vertical => 1,
        }
    }

    /// Whether it overlaps `other` (same axis, stretches meeting).
    pub fn overlaps(&self, other: &ViewBreak) -> bool {
        self.axis() == other.axis() && self.from < other.to && other.from < self.to
    }
}

/// Why a break cannot be made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BreakError {
    /// Both lines at one place.
    Empty,
    /// It overlaps a break of the view.
    Overlaps,
    /// The view holds [`MAX_BREAKS`] already.
    TooMany,
    /// Gap not above zero.
    Gap,
}

impl std::fmt::Display for BreakError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            BreakError::Empty => "the break removes nothing",
            BreakError::Overlaps => "it overlaps another break",
            BreakError::TooMany => "the view has as many breaks as it can hold",
            BreakError::Gap => "the gap must be greater than 0",
        })
    }
}

/// Checks `b` against a view's `breaks` (the one at `skip` left out: an
/// edited break).
pub fn check(b: &ViewBreak, breaks: &[ViewBreak], skip: Option<usize>) -> Result<(), BreakError> {
    if b.to - b.from <= 1e-6 {
        return Err(BreakError::Empty);
    }
    if b.gap <= 0.0 || !b.gap.is_finite() {
        return Err(BreakError::Gap);
    }
    let others = breaks.iter().enumerate().filter(|(k, _)| Some(*k) != skip);
    if others.clone().any(|(_, o)| o.overlaps(b)) {
        return Err(BreakError::Overlaps);
    }
    if skip.is_none() && others.count() >= MAX_BREAKS {
        return Err(BreakError::TooMany);
    }
    Ok(())
}

/// One removed stretch at a view's scale.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cut {
    /// 0: along x, 1: along y.
    pub axis: usize,
    /// The removed stretch's start (view plane, true coordinates).
    pub from: f64,
    /// Its end.
    pub to: f64,
    /// The gap in view-plane mm (sheet gap / scale).
    pub gap: f64,
    /// Its break in the view's list.
    pub index: usize,
}

impl Cut {
    /// How much shorter the view gets.
    fn shrink(&self) -> f64 {
        (self.to - self.from) - self.gap
    }
}

/// A view's breaks at its scale: true view-plane points ↔ the shortened
/// view. `Copy`, so it rides in a [`Placement`](super::annot::Placement).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BreakMap {
    n: usize,
    /// Sorted by axis, then `from`.
    cuts: [Cut; MAX_BREAKS],
}

impl BreakMap {
    /// The map of `breaks` at `scale` (overlapping or empty breaks, and
    /// those past [`MAX_BREAKS`], are left out).
    pub fn new(breaks: &[ViewBreak], scale: f64) -> Self {
        let mut list: Vec<Cut> = Vec::new();
        for (index, b) in breaks.iter().enumerate() {
            if b.suppressed || b.to - b.from <= 1e-9 || list.len() >= MAX_BREAKS {
                continue;
            }
            let c = Cut {
                axis: b.axis(),
                from: b.from,
                to: b.to,
                gap: (b.gap.max(0.0) / scale.max(1e-9)).min(b.to - b.from),
                index,
            };
            if list
                .iter()
                .any(|o| o.axis == c.axis && o.from < c.to && c.from < o.to)
            {
                continue;
            }
            list.push(c);
        }
        list.sort_by(|a, b| a.axis.cmp(&b.axis).then(a.from.total_cmp(&b.from)));
        let mut cuts = [Cut::default(); MAX_BREAKS];
        for (k, c) in list.iter().enumerate() {
            cuts[k] = *c;
        }
        Self {
            n: list.len(),
            cuts,
        }
    }

    /// No breaks.
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// The stretches, by axis and position.
    pub fn cuts(&self) -> &[Cut] {
        &self.cuts[..self.n]
    }

    /// A true point in the shortened view (a point inside a removed
    /// stretch lands in the middle of its gap).
    pub fn shorten(&self, q: [f64; 2]) -> [f64; 2] {
        let mut out = q;
        for c in self.cuts() {
            let u = q[c.axis];
            if u >= c.to {
                out[c.axis] -= c.shrink();
            } else if u > c.from {
                out[c.axis] -= u - c.from - c.gap * 0.5;
            }
        }
        out
    }

    /// A shortened point back in true coordinates (a point in a gap goes
    /// to the nearer end of the removed stretch).
    pub fn lengthen(&self, s: [f64; 2]) -> [f64; 2] {
        let mut out = s;
        for axis in 0..2 {
            let t = s[axis];
            let mut acc = 0.0;
            let mut done = false;
            for c in self.cuts().iter().filter(|c| c.axis == axis) {
                let start = c.from - acc;
                if t <= start {
                    out[axis] = t + acc;
                    done = true;
                    break;
                }
                if t < start + c.gap {
                    out[axis] = if t < start + c.gap * 0.5 {
                        c.from
                    } else {
                        c.to
                    };
                    done = true;
                    break;
                }
                acc += c.shrink();
            }
            if !done {
                out[axis] = t + acc;
            }
        }
        out
    }

    /// Whether `q` lies in a removed stretch (its ends kept).
    pub fn removed(&self, q: [f64; 2]) -> bool {
        self.cuts()
            .iter()
            .any(|c| q[c.axis] > c.from + 1e-9 && q[c.axis] < c.to - 1e-9)
    }

    /// The parts of segment `a`–`b` outside every removed stretch, as
    /// parameter intervals in 0..1.
    fn kept(&self, a: [f64; 2], b: [f64; 2]) -> Vec<(f64, f64)> {
        let mut gone: Vec<(f64, f64)> = Vec::new();
        for c in self.cuts() {
            let (ua, ub) = (a[c.axis], b[c.axis]);
            let d = ub - ua;
            if d.abs() < 1e-12 {
                if ua > c.from && ua < c.to {
                    return Vec::new();
                }
                continue;
            }
            let (t0, t1) = ((c.from - ua) / d, (c.to - ua) / d);
            let (lo, hi) = (t0.min(t1).max(0.0), t0.max(t1).min(1.0));
            if hi > lo {
                gone.push((lo, hi));
            }
        }
        gone.sort_by(|x, y| x.0.total_cmp(&y.0));
        let mut out = Vec::new();
        let mut t = 0.0;
        for (lo, hi) in gone {
            if lo > t {
                out.push((t, lo));
            }
            t = t.max(hi);
        }
        if t < 1.0 {
            out.push((t, 1.0));
        }
        out
    }

    /// A polyline without its removed stretches: the pieces left.
    pub fn clip_polyline(&self, pl: &[[f64; 2]]) -> Vec<Vec<[f64; 2]>> {
        let lerp =
            |a: [f64; 2], b: [f64; 2], t: f64| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
        let mut out = Vec::new();
        let mut cur: Vec<[f64; 2]> = Vec::new();
        let mut flush = |cur: &mut Vec<[f64; 2]>| {
            if cur.len() >= 2 {
                out.push(std::mem::take(cur));
            }
            cur.clear();
        };
        for w in pl.windows(2) {
            let (a, b) = (w[0], w[1]);
            let kept = self.kept(a, b);
            if kept.is_empty() {
                flush(&mut cur);
            }
            for (t0, t1) in kept {
                // A piece goes on from the previous segment only when it
                // starts at this segment's start.
                if t0 > 1e-12 || cur.is_empty() {
                    flush(&mut cur);
                    cur.push(lerp(a, b, t0));
                }
                cur.push(lerp(a, b, t1));
                if t1 < 1.0 - 1e-12 {
                    flush(&mut cur);
                }
            }
        }
        flush(&mut cur);
        out
    }

    /// A closed loop (a cut face) without its removed stretches: one loop
    /// per piece left (each closed).
    pub fn clip_loop(&self, lp: &[[f64; 2]]) -> Vec<Vec<[f64; 2]>> {
        let mut polys = vec![open(lp)];
        for c in self.cuts() {
            let mut next = Vec::new();
            for p in &polys {
                // The side below `from` and the side above `to`.
                for (keep_below, at) in [(true, c.from), (false, c.to)] {
                    let q = half_plane(p, c.axis, at, keep_below);
                    if q.len() >= 3 {
                        next.push(q);
                    }
                }
            }
            polys = next;
        }
        polys
            .into_iter()
            .map(|mut p| {
                if let Some(f) = p.first().copied() {
                    p.push(f);
                }
                p
            })
            .collect()
    }
}

/// A loop without its closing point.
fn open(lp: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut p = lp.to_vec();
    if p.len() > 1 && p.first() == p.last() {
        p.pop();
    }
    p
}

/// Sutherland–Hodgman: the part of polygon `p` with `q[axis] <= at`
/// (`below`) or `>= at`.
fn half_plane(p: &[[f64; 2]], axis: usize, at: f64, below: bool) -> Vec<[f64; 2]> {
    let inside = |q: &[f64; 2]| if below { q[axis] <= at } else { q[axis] >= at };
    let mut out = Vec::new();
    for k in 0..p.len() {
        let a = p[k];
        let b = p[(k + 1) % p.len()];
        let (ia, ib) = (inside(&a), inside(&b));
        if ia {
            out.push(a);
        }
        if ia != ib {
            let t = (at - a[axis]) / (b[axis] - a[axis]);
            out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
        }
    }
    out
}

/// The view's lines with its breaks: the removed stretches clipped out of
/// visible / hidden lines and cut faces, bends inside them left out, and
/// the map kept in [`ViewLines::breaks`] for placing them. Coordinates stay
/// true.
pub fn apply(lines: &ViewLines, breaks: &[ViewBreak], scale: f64) -> ViewLines {
    let map = BreakMap::new(breaks, scale);
    if map.is_empty() {
        return lines.clone();
    }
    let clip = |polys: &[Vec<[f64; 2]>], srcs: &[Option<u32>]| {
        let mut out = Vec::new();
        let mut out_src = Vec::new();
        for (k, pl) in polys.iter().enumerate() {
            for piece in map.clip_polyline(pl) {
                out.push(piece);
                out_src.push(srcs.get(k).copied().flatten());
            }
        }
        (out, out_src)
    };
    let (visible, visible_src) = clip(&lines.visible, &lines.visible_src);
    let (hidden, hidden_src) = clip(&lines.hidden, &lines.hidden_src);
    let section = lines
        .section
        .iter()
        .flat_map(|lp| map.clip_loop(lp))
        .collect();
    let bends = lines
        .bends
        .iter()
        .filter(|b| !map.removed(b.middle()))
        .cloned()
        .collect();
    ViewLines {
        visible,
        visible_src,
        hidden,
        hidden_src,
        section,
        bends,
        breaks: map,
        ..lines.clone()
    }
}

/// Where the geometry crosses the line `q[axis] = at`: the span along the
/// other axis (view plane), from the visible lines.
pub fn span_at(lines: &ViewLines, axis: usize, at: f64) -> Option<[f64; 2]> {
    let other = 1 - axis;
    let mut lo = f64::MAX;
    let mut hi = f64::MIN;
    let tol = 1e-6 * (1.0 + at.abs());
    for pl in &lines.visible {
        for w in pl.windows(2) {
            let (a, b) = (w[0], w[1]);
            let (ua, ub) = (a[axis], b[axis]);
            let mut take = |v: f64| {
                lo = lo.min(v);
                hi = hi.max(v);
            };
            if (ua - at).abs() <= tol {
                take(a[other]);
            }
            if (ub - at).abs() <= tol {
                take(b[other]);
            }
            if (ua - at) * (ub - at) < 0.0 {
                let t = (at - ua) / (ub - ua);
                take(a[other] + (b[other] - a[other]) * t);
            }
        }
    }
    (hi >= lo).then_some([lo, hi])
}

/// The break lines of break `b` (sheet mm), one at each piece's end:
/// `ends[k]` are the sheet points of the geometry's two edges across the
/// break at piece end k (the line runs from the first to the second).
pub fn break_lines(b: &ViewBreak, ends: [([f64; 2], [f64; 2]); 2]) -> Vec<Vec<[f64; 2]>> {
    let mut out = Vec::new();
    let d = f64::from(
        b.display
            .clamp(*DISPLAY_RANGE.start(), *DISPLAY_RANGE.end()),
    );
    for (p0, p1) in ends {
        let along = [p1[0] - p0[0], p1[1] - p0[1]];
        let len = along[0].hypot(along[1]);
        if len < 1e-9 {
            continue;
        }
        let u = [along[0] / len, along[1] / len];
        // Square to the line, along the shortened axis.
        let n = [u[1], -u[0]];
        let at = |s: f64, off: f64| [p0[0] + u[0] * s + n[0] * off, p0[1] + u[1] * s + n[1] * off];
        match b.style {
            BreakStyle::Rectangular => {
                // A wave across the geometry: more waves as Display grows.
                let amp = b.gap * 0.2;
                let pitch = (20.0 - 1.6 * d).max(2.0);
                let waves = (len / pitch).round().max(1.0);
                let steps = (waves * 16.0) as usize;
                out.push(
                    (0..=steps)
                        .map(|k| {
                            let s = len * k as f64 / steps as f64;
                            let ph = std::f64::consts::TAU * waves * s / len;
                            at(s, amp * ph.sin())
                        })
                        .collect(),
                );
            }
            BreakStyle::Structural => {
                // A straight line past the geometry with Z symbols, taller
                // as Display grows.
                let over = b.gap * 0.5;
                let h = b.gap * 0.06 * d;
                let w = b.gap * 0.6;
                let n_sym = usize::from(b.symbols.clamp(1, 3));
                let mut pts = vec![at(-over, 0.0)];
                for k in 0..n_sym {
                    let s = len * (k as f64 + 0.5) / n_sym as f64;
                    pts.push(at(s - w * 0.5, 0.0));
                    pts.push(at(s - w / 6.0, h));
                    pts.push(at(s + w / 6.0, -h));
                    pts.push(at(s + w * 0.5, 0.0));
                }
                pts.push(at(len + over, 0.0));
                out.push(pts);
            }
        }
    }
    out
}
