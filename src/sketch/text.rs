//! Sketch text (`docs/spec/1.11-sketch-text.md`).
//!
//! A text is laid out once, when it is typed or edited, and its glyph outlines are stored in the
//! sketch: a part regenerates the same on a machine without the font (rule 8). It stands on a
//! construction line of the sketch (its *baseline frame*): the line's start is the text's top
//! left, its direction the writing direction, so constraints and dimensions on that line place
//! and turn the text. Each glyph gives closed regions ([`Sketch::regions`]), which Extrude,
//! Revolve, Emboss and the others take like any other profile.
//!
//! The rules follow Inventor's: the size is the height of the capital letters whatever the font,
//! the top left is where the ink of the first line starts, no kerning.

use serde::{Deserialize, Serialize};

use super::profile::{ProfileCurve, ProfileEdge, ProfileLoop, Region};
use super::{Entity, EntityId, Point2, Sketch, SketchError, Style, Vector2};
use crate::kernel::Real;

/// Distance between baselines at single spacing, as a factor of the size (Arial in Inventor).
const LINE_PITCH: Real = 1.5737;

/// Horizontal alignment of the lines.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextAlign {
    /// From the frame's start.
    #[default]
    Left,
    /// Centred on the frame's start.
    Center,
    /// Ending at the frame's start.
    Right,
}

/// What the user chose for a text.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextSpec {
    /// The text; `\n` starts a line.
    pub text: String,
    /// Font family.
    pub font: String,
    /// Bold face.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bold: bool,
    /// Italic face.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub italic: bool,
    /// Height of the capital letters (mm).
    pub size: Real,
    /// Width factor (1 = as designed).
    #[serde(default = "one")]
    pub stretch: Real,
    /// Line spacing factor (1 = single).
    #[serde(default = "one")]
    pub spacing: Real,
    /// Alignment of the lines.
    #[serde(default)]
    pub align: TextAlign,
}

fn one() -> Real {
    1.0
}

impl TextSpec {
    /// A left-aligned text in `font`.
    pub fn new(text: impl Into<String>, font: impl Into<String>, size: Real) -> Self {
        Self {
            text: text.into(),
            font: font.into(),
            bold: false,
            italic: false,
            size,
            stretch: 1.0,
            spacing: 1.0,
            align: TextAlign::Left,
        }
    }
}

/// One piece of a glyph's outline, in the text's own coordinates (x along the writing direction,
/// y up, the origin at the top left).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum TextSeg {
    /// A straight piece.
    Line([[Real; 2]; 2]),
    /// A cubic Bézier piece.
    Cubic([[Real; 2]; 4]),
}

impl TextSeg {
    fn start(&self) -> [Real; 2] {
        match self {
            TextSeg::Line(p) => p[0],
            TextSeg::Cubic(p) => p[0],
        }
    }

    fn points(&self) -> &[[Real; 2]] {
        match self {
            TextSeg::Line(p) => p,
            TextSeg::Cubic(p) => p,
        }
    }

    fn map(&self, f: impl Fn([Real; 2]) -> [Real; 2]) -> Self {
        match self {
            TextSeg::Line(p) => TextSeg::Line(p.map(f)),
            TextSeg::Cubic(p) => TextSeg::Cubic(p.map(f)),
        }
    }

    /// Points along the piece, the last one left out (the next piece starts there).
    fn flat(&self, out: &mut Vec<[Real; 2]>) {
        match self {
            TextSeg::Line(p) => out.push(p[0]),
            TextSeg::Cubic(p) => {
                for k in 0..8 {
                    let t = k as Real / 8.0;
                    let m = 1.0 - t;
                    let w = [m * m * m, 3.0 * m * m * t, 3.0 * m * t * t, t * t * t];
                    let x = |i: usize| (0..4).map(|j| w[j] * p[j][i]).sum();
                    out.push([x(0), x(1)]);
                }
            }
        }
    }
}

/// The closed loops of one glyph.
pub type Glyph = Vec<Vec<TextSeg>>;

/// A text of a sketch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SketchText {
    /// The construction line it stands on (also the `source` of its profile edges).
    pub line: EntityId,
    /// What was asked for.
    pub spec: TextSpec,
    /// The outlines made from the font (never read from the font again until it is edited).
    pub glyphs: Vec<Glyph>,
    /// Width of the longest line (mm).
    pub width: Real,
    /// From the top to the lowest ink (mm, positive).
    pub height: Real,
}

/// A font's faces and glyphs, as far as the layout needs them.
pub trait GlyphSource {
    /// The glyph of `c` scaled so that a capital letter is 1 high: its advance and its loops
    /// (x right, y up, baseline at 0). `None`: the font does not have it.
    fn glyph(&self, c: char) -> Option<(Real, Glyph)>;
}

/// Lays `spec` out from `source`.
pub fn layout(
    spec: &TextSpec,
    source: &dyn GlyphSource,
) -> Result<(Vec<Glyph>, Real, Real), String> {
    if !(spec.size.is_finite() && spec.size > 0.0) {
        return Err("the size must be positive".into());
    }
    if !(spec.stretch.is_finite() && spec.stretch > 0.0) {
        return Err("the width factor must be positive".into());
    }
    if !(spec.spacing.is_finite() && spec.spacing > 0.0) {
        return Err("the line spacing must be positive".into());
    }
    if spec.text.trim().is_empty() {
        return Err("type the text".into());
    }
    // Each line's glyphs at their advance, baseline at 0.
    let mut lines: Vec<(Real, Vec<Glyph>)> = Vec::new();
    for line in spec.text.split('\n') {
        let line = line.trim_end_matches('\r');
        let (mut x, mut gs) = (0.0, Vec::new());
        for c in line.chars() {
            let Some((advance, glyph)) = source.glyph(c) else {
                return Err(format!("the font {} has no character {c}", spec.font));
            };
            let at = x;
            let (sx, s) = (spec.size * spec.stretch, spec.size);
            let moved: Glyph = glyph
                .iter()
                .map(|l| {
                    l.iter()
                        .map(|seg| seg.map(|p| [at + p[0] * sx, p[1] * s]))
                        .collect()
                })
                .collect();
            if !moved.is_empty() {
                gs.push(moved);
            }
            x += advance * sx;
        }
        lines.push((x, gs));
    }
    let width = lines.iter().map(|l| l.0).fold(0.0, Real::max);
    let pitch = LINE_PITCH * spec.size * spec.spacing;
    let mut out: Vec<Glyph> = Vec::new();
    for (k, (w, gs)) in lines.into_iter().enumerate() {
        let dx = match spec.align {
            TextAlign::Left => 0.0,
            TextAlign::Center => -w / 2.0,
            TextAlign::Right => -w,
        };
        let dy = -(k as Real) * pitch;
        for g in gs {
            out.push(
                g.iter()
                    .map(|l| {
                        l.iter()
                            .map(|s| s.map(|p| [p[0] + dx, p[1] + dy]))
                            .collect()
                    })
                    .collect(),
            );
        }
    }
    if out.is_empty() {
        return Err("the text has nothing to draw".into());
    }
    // The ink's top is the origin.
    let ys = || {
        out.iter()
            .flatten()
            .flatten()
            .flat_map(|s| s.points().iter().map(|p| p[1]))
    };
    let top = ys().fold(Real::NEG_INFINITY, Real::max);
    let glyphs: Vec<Glyph> = out
        .iter()
        .map(|g| {
            g.iter()
                .map(|l| l.iter().map(|s| s.map(|p| [p[0], p[1] - top])).collect())
                .collect()
        })
        .collect();
    let low = ys().fold(Real::INFINITY, Real::min) - top;
    Ok((glyphs, width, -low))
}

/// How far a letter's wall may stand from the font's curve, as a factor of the size.
const WALL_TOL: Real = 2e-4;

fn bez(p: &[[Real; 2]; 4], t: Real) -> [Real; 2] {
    let m = 1.0 - t;
    let w = [m * m * m, 3.0 * m * m * t, 3.0 * m * t * t, t * t * t];
    [
        (0..4).map(|j| w[j] * p[j][0]).sum(),
        (0..4).map(|j| w[j] * p[j][1]).sum(),
    ]
}

/// A font's cubic as the walls of a profile: arcs through three points of each piece, within
/// `tol` of the curve (lines where it is straight). The kernel's Booleans on B-spline walls took
/// 9.6 s for one word on a plate and failed over its edge; with arcs 3.4 s and exact, with lines
/// 106 s for one letter over the edge.
fn walls_of(
    p: &[[Real; 2]; 4],
    (t0, t1): (Real, Real),
    tol: Real,
    depth: usize,
    place: &impl Fn([Real; 2]) -> Point2,
    out: &mut Vec<ProfileCurve>,
) {
    let (pa, pm, pb) = (
        place(bez(p, t0)),
        place(bez(p, (t0 + t1) / 2.0)),
        place(bez(p, t1)),
    );
    let chord = pb - pa;
    let len = chord.norm();
    let samples = [0.125, 0.25, 0.375, 0.625, 0.75, 0.875];
    let at = |f: Real| place(bez(p, t0 + (t1 - t0) * f));
    let off = |q: Point2| ((q - pa).perp(&chord) / len.max(1e-12)).abs();
    if samples.iter().all(|f| off(at(*f)) <= tol) {
        out.push(ProfileCurve::Line { a: pa, b: pb });
        return;
    }
    // The circle through the piece's ends and middle.
    let d = 2.0 * (pa.x * (pm.y - pb.y) + pm.x * (pb.y - pa.y) + pb.x * (pa.y - pm.y));
    if d.abs() > 1e-12 {
        let sq = |q: Point2| q.x * q.x + q.y * q.y;
        let c = Point2::new(
            (sq(pa) * (pm.y - pb.y) + sq(pm) * (pb.y - pa.y) + sq(pb) * (pa.y - pm.y)) / d,
            (sq(pa) * (pb.x - pm.x) + sq(pm) * (pa.x - pb.x) + sq(pb) * (pm.x - pa.x)) / d,
        );
        let r = (pa - c).norm();
        if samples
            .iter()
            .all(|f| ((at(*f) - c).norm() - r).abs() <= tol)
        {
            let ang = |q: Point2| (q.y - c.y).atan2(q.x - c.x);
            let wrap = |x: Real| {
                (x + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI
            };
            let sweep = wrap(ang(pm) - ang(pa)) + wrap(ang(pb) - ang(pm));
            out.push(ProfileCurve::Arc {
                c,
                r,
                a0: ang(pa),
                sweep,
            });
            return;
        }
    }
    if depth == 0 {
        out.push(ProfileCurve::Line { a: pa, b: pb });
        return;
    }
    let mid = (t0 + t1) / 2.0;
    walls_of(p, (t0, mid), tol, depth - 1, place, out);
    walls_of(p, (mid, t1), tol, depth - 1, place, out);
}

/// The signed area of a loop of points.
fn area(pts: &[[Real; 2]]) -> Real {
    (0..pts.len())
        .map(|k| {
            let (a, b) = (pts[k], pts[(k + 1) % pts.len()]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<Real>()
        / 2.0
}

fn inside(p: [Real; 2], poly: &[[Real; 2]]) -> bool {
    let mut odd = false;
    for k in 0..poly.len() {
        let (a, b) = (poly[k], poly[(k + 1) % poly.len()]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0])
        {
            odd = !odd;
        }
    }
    odd
}

impl SketchText {
    /// The glyphs placed on the frame `(origin, direction)`: closed regions, outer loops
    /// counter-clockwise, holes clockwise.
    pub fn regions_at(&self, origin: Point2, dir: Vector2) -> Vec<Region> {
        let v = Vector2::new(-dir.y, dir.x);
        let place = |p: [Real; 2]| origin + dir * p[0] + v * p[1];
        let tol = self.spec.size * WALL_TOL;
        let make_loop = |l: &[TextSeg], ccw: bool, flat: &[[Real; 2]]| -> ProfileLoop {
            let flip = (area(flat) > 0.0) != ccw;
            let mut curves: Vec<ProfileCurve> = Vec::new();
            for s in l {
                match s {
                    TextSeg::Line(p) => curves.push(ProfileCurve::Line {
                        a: place(p[0]),
                        b: place(p[1]),
                    }),
                    TextSeg::Cubic(p) => walls_of(p, (0.0, 1.0), tol, 6, &place, &mut curves),
                }
            }
            if flip {
                curves = curves.iter().rev().map(ProfileCurve::reversed).collect();
            }
            ProfileLoop {
                edges: curves
                    .into_iter()
                    .map(|curve| ProfileEdge {
                        source: self.line,
                        curve,
                    })
                    .collect(),
            }
        };
        let mut out = Vec::new();
        for glyph in &self.glyphs {
            let flats: Vec<Vec<[Real; 2]>> = glyph
                .iter()
                .map(|l| {
                    let mut f = Vec::new();
                    l.iter().for_each(|s| s.flat(&mut f));
                    f
                })
                .collect();
            // The loop's depth decides: inside an even number of others it is an outline, inside
            // an odd number a hole (fonts wind either way round).
            let depth: Vec<usize> = (0..glyph.len())
                .map(|i| {
                    let at = glyph[i].first().map_or([0.0, 0.0], TextSeg::start);
                    (0..glyph.len())
                        .filter(|&j| j != i && inside(at, &flats[j]))
                        .count()
                })
                .collect();
            let mut regions: Vec<(usize, Region)> = Vec::new();
            for i in 0..glyph.len() {
                if depth[i].is_multiple_of(2) {
                    regions.push((
                        i,
                        Region {
                            outer: make_loop(&glyph[i], true, &flats[i]),
                            holes: Vec::new(),
                        },
                    ));
                }
            }
            for i in 0..glyph.len() {
                if !depth[i].is_multiple_of(2) {
                    let at = glyph[i].first().map_or([0.0, 0.0], TextSeg::start);
                    // The deepest outline around it.
                    let host = regions
                        .iter_mut()
                        .filter(|(o, _)| inside(at, &flats[*o]))
                        .max_by_key(|(o, _)| depth[*o]);
                    if let Some((_, r)) = host {
                        r.holes.push(make_loop(&glyph[i], false, &flats[i]));
                    }
                }
            }
            out.extend(regions.into_iter().map(|(_, r)| r));
        }
        out
    }

    /// Polylines of the outlines on the frame, for drawing.
    pub fn polylines_at(&self, origin: Point2, dir: Vector2) -> Vec<Vec<Point2>> {
        let v = Vector2::new(-dir.y, dir.x);
        self.glyphs
            .iter()
            .flatten()
            .map(|l| {
                let mut f = Vec::new();
                l.iter().for_each(|s| s.flat(&mut f));
                if let Some(first) = f.first().copied() {
                    f.push(first);
                }
                f.iter().map(|p| origin + dir * p[0] + v * p[1]).collect()
            })
            .collect()
    }
}

impl Sketch {
    /// The texts of the sketch.
    pub fn texts(&self) -> &[SketchText] {
        &self.texts
    }

    /// The frame of text `t`: its top left and writing direction (`None`: its line is gone).
    pub fn text_frame(&self, t: &SketchText) -> Option<(Point2, Vector2)> {
        let Some(Entity::Line { start, end }) = self.entity(t.line) else {
            return None;
        };
        let (a, b) = (self.point(*start)?.position, self.point(*end)?.position);
        let d = (b - a).try_normalize(1e-12)?;
        Some((a, d))
    }

    /// Adds a text with its top left at `at`, written at `angle` radians. Returns its line.
    pub fn add_text(
        &mut self,
        at: Point2,
        angle: Real,
        spec: TextSpec,
        source: &dyn GlyphSource,
    ) -> Result<EntityId, SketchError> {
        let (glyphs, width, height) = layout(&spec, source).map_err(SketchError::InvalidText)?;
        let len = width.max(spec.size);
        let end = at + Vector2::new(angle.cos(), angle.sin()) * len;
        let line = self.add_line(at, end);
        self.set_style(line, Style::Construction)?;
        self.texts.push(SketchText {
            line,
            spec,
            glyphs,
            width,
            height,
        });
        Ok(line)
    }

    /// Replaces text `line`'s content (Edit Text); on an error nothing changes.
    pub fn set_text(
        &mut self,
        line: EntityId,
        spec: TextSpec,
        source: &dyn GlyphSource,
    ) -> Result<(), SketchError> {
        let (glyphs, width, height) = layout(&spec, source).map_err(SketchError::InvalidText)?;
        let t = self
            .texts
            .iter_mut()
            .find(|t| t.line == line)
            .ok_or(SketchError::InvalidValue("the text no longer exists"))?;
        (t.spec, t.glyphs, t.width, t.height) = (spec, glyphs, width, height);
        Ok(())
    }

    /// Text regions, after the curves' regions.
    pub(super) fn text_regions(&self) -> Vec<Region> {
        self.texts
            .iter()
            .filter_map(|t| self.text_frame(t).map(|(o, d)| t.regions_at(o, d)))
            .flatten()
            .collect()
    }

    /// The text under sketch point `p`, within `tol` of its box.
    pub fn text_at(&self, p: Point2, tol: Real) -> Option<EntityId> {
        self.texts.iter().find_map(|t| {
            let (o, d) = self.text_frame(t)?;
            let r = p - o;
            let (x, y) = (r.dot(&d), r.dot(&Vector2::new(-d.y, d.x)));
            let (lo, hi) = match t.spec.align {
                TextAlign::Left => (0.0, t.width),
                TextAlign::Center => (-t.width / 2.0, t.width / 2.0),
                TextAlign::Right => (-t.width, 0.0),
            };
            (x >= lo - tol && x <= hi + tol && y <= tol && y >= -t.height - tol).then_some(t.line)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sketch::Region;

    /// A font of two glyphs: a square `I` and a ring `O`.
    struct Blocks;

    fn square(a: [Real; 2], b: [Real; 2]) -> Vec<TextSeg> {
        let p = [a, [b[0], a[1]], b, [a[0], b[1]]];
        (0..4)
            .map(|k| TextSeg::Line([p[k], p[(k + 1) % 4]]))
            .collect()
    }

    impl GlyphSource for Blocks {
        fn glyph(&self, c: char) -> Option<(Real, Glyph)> {
            match c {
                'I' => Some((0.5, vec![square([0.1, 0.0], [0.4, 1.0])])),
                'O' => Some((
                    1.0,
                    // Wound the way TrueType fonts do: the outline clockwise.
                    vec![
                        square([0.0, 1.0], [0.9, 0.0]),
                        square([0.3, 0.3], [0.6, 0.7]),
                    ],
                )),
                ' ' => Some((0.3, Vec::new())),
                _ => None,
            }
        }
    }

    fn area_of(r: &[Region]) -> Real {
        r.iter().map(Region::area).sum()
    }

    #[test]
    fn size_is_the_capital_height_and_the_top_left_is_the_origin() {
        let (g, w, h) = layout(&TextSpec::new("I", "Blocks", 10.0), &Blocks).unwrap();
        assert_eq!(g.len(), 1);
        assert!((w - 5.0).abs() < 1e-9);
        assert!((h - 10.0).abs() < 1e-9);
    }

    #[test]
    fn regions_have_outlines_and_holes_on_the_frame() {
        let mut s = Sketch::new(crate::kernel::Plane::xy());
        s.add_text(
            Point2::new(0.0, 0.0),
            0.0,
            TextSpec::new("IO", "Blocks", 10.0),
            &Blocks,
        )
        .unwrap();
        let r = s.regions();
        assert_eq!(r.len(), 2);
        // I: 3 x 10; O: 9 x 10 less 3 x 4.
        assert!(
            (area_of(&r) - (30.0 + 90.0 - 12.0)).abs() < 1e-9,
            "{}",
            area_of(&r)
        );
        assert_eq!(r.iter().map(|x| x.holes.len()).sum::<usize>(), 1);
        // Below the frame's start, to the right of it.
        for p in r.iter().flat_map(|x| x.outer.polygon(0.1)) {
            assert!(
                p.y <= 1e-9 && p.y >= -10.0 - 1e-9 && p.x >= 0.0 && p.x <= 15.0,
                "{p:?}"
            );
        }
    }

    #[test]
    fn the_line_turns_and_places_the_text() {
        let mut s = Sketch::new(crate::kernel::Plane::xy());
        s.add_text(
            Point2::new(5.0, 5.0),
            std::f64::consts::FRAC_PI_2,
            TextSpec::new("I", "Blocks", 10.0),
            &Blocks,
        )
        .unwrap();
        let r = s.regions();
        assert!((area_of(&r) - 30.0).abs() < 1e-9);
        // Written upwards: the ink runs up from y = 5, to the right of the frame.
        for p in r[0].outer.polygon(0.1) {
            assert!(p.y >= 5.0 - 1e-9 && p.x >= 5.0 - 1e-9, "{p:?}");
        }
    }

    #[test]
    fn stretch_spacing_and_alignment() {
        let mut spec = TextSpec::new("I\nI", "Blocks", 10.0);
        spec.stretch = 0.5;
        spec.spacing = 2.0;
        spec.align = TextAlign::Right;
        let (g, w, h) = layout(&spec, &Blocks).unwrap();
        assert_eq!(g.len(), 2);
        assert!((w - 2.5).abs() < 1e-9);
        assert!((h - (10.0 + 2.0 * LINE_PITCH * 10.0)).abs() < 1e-9, "{h}");
        // Ends at the frame's start.
        for p in g.iter().flatten().flatten().flat_map(|s| s.points()) {
            assert!(p[0] <= 1e-9, "{p:?}");
        }
    }

    #[test]
    fn errors_leave_the_sketch_unchanged() {
        let mut s = Sketch::new(crate::kernel::Plane::xy());
        let line = s
            .add_text(
                Point2::origin(),
                0.0,
                TextSpec::new("I", "Blocks", 10.0),
                &Blocks,
            )
            .unwrap();
        let before = s.texts().to_vec();
        assert!(
            s.set_text(line, TextSpec::new("Ж", "Blocks", 10.0), &Blocks)
                .is_err()
        );
        assert!(
            s.set_text(line, TextSpec::new("  ", "Blocks", 10.0), &Blocks)
                .is_err()
        );
        assert!(
            s.set_text(line, TextSpec::new("I", "Blocks", 0.0), &Blocks)
                .is_err()
        );
        assert_eq!(s.texts(), &before[..]);
    }

    #[test]
    fn removing_its_line_removes_the_text_and_a_save_keeps_it() {
        let mut s = Sketch::new(crate::kernel::Plane::xy());
        let line = s
            .add_text(
                Point2::origin(),
                0.0,
                TextSpec::new("O", "Blocks", 10.0),
                &Blocks,
            )
            .unwrap();
        let json = serde_json::to_string(&s).unwrap();
        let back: Sketch = serde_json::from_str(&json).unwrap();
        assert_eq!(back.regions().len(), 1);
        s.remove_entity(line).unwrap();
        assert!(s.texts().is_empty());
        assert!(s.regions().is_empty());
    }
}
