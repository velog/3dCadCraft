//! **Hatch patterns from PAT files** (spec 4.6 §3.9): AutoCAD's pattern format, read into hatch
//! families that a drawing's hatch style keeps (rule 8: the drawing draws the same without the
//! file), and the families drawn as clipped dashed lines.
//!
//! A PAT file holds patterns, each a header `*NAME[, description]` and one line per family:
//! `angle, x-origin, y-origin, delta-x, delta-y [, dash-1, dash-2, …]` — the family's lines run at
//! `angle` through the origin, each next one moved `delta-x` along the line and `delta-y` across
//! it; dashes are pen-down lengths (positive), gaps (negative) and dots (0). After SolidCraft's PAT
//! import (`crates/io/src/pat.rs`, MIT OR Apache-2.0); the code is ours.

use serde::{Deserialize, Serialize};

/// The most families in one pattern, and dashes in one family (as AutoCAD's limits).
const MAX_FAMILIES: usize = 64;
const MAX_DASHES: usize = 16;

/// One family of parallel lines of a pattern (mm, degrees).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HatchFamily {
    /// The lines' direction (degrees).
    pub angle: f64,
    /// A point the first line passes through.
    pub origin: [f64; 2],
    /// Each next line: moved `delta[0]` along the line and `delta[1]` across it.
    pub delta: [f64; 2],
    /// Dash pattern: lengths drawn (> 0), gaps (< 0), dots (0); empty = continuous.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dashes: Vec<f64>,
}

/// A pattern of a PAT file.
#[derive(Clone, Debug, PartialEq)]
pub struct PatPattern {
    /// Its name.
    pub name: String,
    /// Its description.
    pub description: String,
    /// Its families of lines.
    pub families: Vec<HatchFamily>,
}

/// Reads every pattern of a PAT file; `unit` is the file's length unit in mm (1 for metric
/// files such as `acadiso.pat`, 25.4 for inch ones). Comments (`;`) and blank lines are skipped.
pub fn read_pat(text: &str, unit: f64) -> Result<Vec<PatPattern>, String> {
    let mut out: Vec<PatPattern> = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.split(';').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if let Some(head) = line.strip_prefix('*') {
            let (name, desc) = head.split_once(',').unwrap_or((head, ""));
            out.push(PatPattern {
                name: name.trim().to_string(),
                description: desc.trim().to_string(),
                families: Vec::new(),
            });
            continue;
        }
        let Some(p) = out.last_mut() else {
            return Err(format!("line {}: a family before any *pattern name", n + 1));
        };
        let v: Vec<f64> = line
            .split(',')
            .map(|x| x.trim().parse::<f64>())
            .collect::<Result<_, _>>()
            .map_err(|_| format!("line {}: not a list of numbers", n + 1))?;
        if v.len() < 5 || v.iter().any(|x| !x.is_finite()) {
            return Err(format!(
                "line {}: a family needs angle, x, y, delta-x, delta-y",
                n + 1
            ));
        }
        if p.families.len() >= MAX_FAMILIES || v.len() - 5 > MAX_DASHES {
            return Err(format!("line {}: too many families or dashes", n + 1));
        }
        if v[4].abs() < 1e-9 {
            return Err(format!("line {}: delta-y 0 puts every line on one", n + 1));
        }
        p.families.push(HatchFamily {
            angle: v[0],
            origin: [v[1] * unit, v[2] * unit],
            delta: [v[3] * unit, v[4] * unit],
            dashes: v[5..].iter().map(|d| d * unit).collect(),
        });
    }
    out.retain(|p| !p.families.is_empty());
    Ok(out)
}

/// The pieces of family `f` inside the even-odd region of `loops` (sheet mm), the pattern turned
/// by `turn` degrees and scaled by `scale` about the sheet origin.
pub fn family_segments(
    loops: &[Vec<[f64; 2]>],
    f: &HatchFamily,
    turn: f64,
    scale: f64,
) -> Vec<([f64; 2], [f64; 2])> {
    let mut out = Vec::new();
    let (dx, dy) = (f.delta[0] * scale, f.delta[1] * scale);
    if loops.is_empty() || dy.abs() < 1e-6 || scale.is_nan() || scale <= 0.0 {
        return out;
    }
    let a = (f.angle + turn).to_radians();
    let (s, c) = a.sin_cos();
    let (ts, tc) = turn.to_radians().sin_cos();
    let o = [f.origin[0] * scale, f.origin[1] * scale];
    let o = [tc * o[0] - ts * o[1], ts * o[0] + tc * o[1]];
    // Frame of the family: u along the lines, v across, from the origin.
    let to = |p: [f64; 2]| {
        let (x, y) = (p[0] - o[0], p[1] - o[1]);
        [c * x + s * y, -s * x + c * y]
    };
    let back = |q: [f64; 2]| [o[0] + c * q[0] - s * q[1], o[1] + s * q[0] + c * q[1]];
    let edges: Vec<([f64; 2], [f64; 2])> = loops
        .iter()
        .flat_map(|lp| lp.windows(2).map(|w| (to(w[0]), to(w[1]))))
        .collect();
    let (lo, hi) = edges.iter().fold((f64::MAX, f64::MIN), |(lo, hi), (p, q)| {
        (lo.min(p[1]).min(q[1]), hi.max(p[1]).max(q[1]))
    });
    if lo > hi {
        return out;
    }
    let step = dy.abs();
    let dashes: Vec<f64> = f.dashes.iter().map(|d| d * scale).collect();
    let period: f64 = dashes.iter().map(|d| d.abs()).sum();
    let (k0, k1) = ((lo / step).floor() as i64, (hi / step).ceil() as i64);
    for k in k0..=k1 {
        let v = k as f64 * step;
        // Line k is moved k·dx along (when dy is negative, k counts the other way).
        let shift = k as f64 * dx * dy.signum();
        let mut xs: Vec<f64> = edges
            .iter()
            .filter(|(p, q)| (p[1] > v) != (q[1] > v))
            .map(|(p, q)| p[0] + (v - p[1]) / (q[1] - p[1]) * (q[0] - p[0]))
            .collect();
        xs.sort_by(f64::total_cmp);
        for pair in xs.chunks(2) {
            let [x0, x1] = pair else { continue };
            if period < 1e-9 {
                out.push((back([*x0, v]), back([*x1, v])));
                continue;
            }
            // Walk the dash pattern from the period at or before x0.
            let mut u = shift + ((x0 - shift) / period).floor() * period;
            'walk: while u < *x1 {
                for d in &dashes {
                    let end = u + d.abs();
                    if *d >= 0.0 {
                        // A dot is drawn as a short dash of 0.1 mm.
                        let e = if *d == 0.0 { u + 0.1 } else { end };
                        let (a0, a1) = (u.max(*x0), e.min(*x1));
                        if a1 > a0 {
                            out.push((back([a0, v]), back([a1, v])));
                        }
                    }
                    u = end;
                    if u >= *x1 {
                        break 'walk;
                    }
                }
                if out.len() > 200_000 {
                    return out;
                }
            }
        }
        if out.len() > 200_000 {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
;; A few patterns in the PAT format (written for this test).
*LINES45, Lines at 45 degrees
45, 0,0, 0,3.175
*GRID, Two families
0, 0,0, 0,5
90, 0,0, 0,5
*DASHED, Dashed lines
0, 0,0, 2,4, 6,-2
";

    fn square(side: f64) -> Vec<Vec<[f64; 2]>> {
        vec![vec![
            [0.0, 0.0],
            [side, 0.0],
            [side, side],
            [0.0, side],
            [0.0, 0.0],
        ]]
    }

    fn length(segs: &[([f64; 2], [f64; 2])]) -> f64 {
        segs.iter()
            .map(|(a, b)| (a[0] - b[0]).hypot(a[1] - b[1]))
            .sum()
    }

    #[test]
    fn reads_patterns_and_refuses_broken_lines() {
        let p = read_pat(SAMPLE, 1.0).unwrap();
        assert_eq!(p.len(), 3);
        assert_eq!(p[0].name, "LINES45");
        assert_eq!(p[0].description, "Lines at 45 degrees");
        assert_eq!(p[1].families.len(), 2);
        assert_eq!(p[2].families[0].dashes, vec![6.0, -2.0]);
        // Inch files: every length times 25.4, angles unchanged.
        let i = read_pat(SAMPLE, 25.4).unwrap();
        assert!((i[0].families[0].delta[1] - 3.175 * 25.4).abs() < 1e-9);
        assert_eq!(i[0].families[0].angle, 45.0);
        assert!(read_pat("0, 0,0, 0,3\n", 1.0).is_err());
        assert!(read_pat("*A\n0, 0, x, 0, 3\n", 1.0).is_err());
        assert!(read_pat("*A\n0, 0,0, 1,0\n", 1.0).is_err());
    }

    /// Lines fill a square with the length its area over the spacing; a grid twice; a dashed
    /// family three quarters of that (6 drawn of every 8).
    #[test]
    fn families_fill_the_region() {
        let p = read_pat(SAMPLE, 1.0).unwrap();
        let sq = square(100.0);
        let total = |pat: &PatPattern, turn: f64| -> f64 {
            pat.families
                .iter()
                .map(|f| length(&family_segments(&sq, f, turn, 1.0)))
                .sum()
        };
        let lines = total(&p[0], 0.0);
        assert!((lines - 10_000.0 / 3.175).abs() < 0.02 * lines, "{lines}");
        let grid = total(&p[1], 0.0);
        assert!((grid - 2.0 * 10_000.0 / 5.0).abs() < 0.03 * grid, "{grid}");
        let dashed = total(&p[2], 0.0);
        let full = 10_000.0 / 4.0;
        assert!((dashed - 0.75 * full).abs() < 0.03 * full, "{dashed}");
        // Turned and scaled: the same pattern twice as coarse is half the length.
        let coarse: f64 = p[0]
            .families
            .iter()
            .map(|f| length(&family_segments(&sq, f, 30.0, 2.0)))
            .sum();
        assert!((coarse - lines / 2.0).abs() < 0.03 * lines, "{coarse}");
        // Every piece stays inside the square.
        for f in &p[2].families {
            for (a, b) in family_segments(&sq, f, 17.0, 1.0) {
                for q in [a, b] {
                    assert!((-1e-9..=100.0 + 1e-9).contains(&q[0]), "{q:?}");
                    assert!((-1e-9..=100.0 + 1e-9).contains(&q[1]), "{q:?}");
                }
            }
        }
    }
}
