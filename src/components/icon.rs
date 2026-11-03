//! A bearing family's picture: its section through the axis, the way a catalogue draws it, since
//! from outside every bearing is the same ring. Two rows show as two rows. Drawn on the CPU like
//! `part::thumbnail`, so it works headless.

use crate::kernel::Real;

/// How a bearing's rolling elements sit, from its type (`types.csv`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BearingStyle {
    /// One row of balls in grooves.
    Ball,
    /// Two rows of balls.
    TwoBalls,
    /// One row of cylinders, parallel to the axis.
    Roller,
    /// A thin section: long thin needles.
    Needle,
    /// Cone rollers: rings and rollers on a slant.
    Tapered,
    /// Two rows of barrel rollers.
    TwoBarrels,
    /// Balls between two washers, along the axis.
    Thrust,
}

impl BearingStyle {
    /// The style of a bearing type's name; `None`: not a bearing with rolling elements.
    pub fn of_type(name: &str) -> Option<BearingStyle> {
        Some(match name {
            "Deep Groove Ball" | "Angular Contact Ball" => BearingStyle::Ball,
            "Self-Aligning Ball" => BearingStyle::TwoBalls,
            "Cylindrical Roller" => BearingStyle::Roller,
            "Needle Roller" => BearingStyle::Needle,
            "Tapered Roller" => BearingStyle::Tapered,
            "Spherical Roller" => BearingStyle::TwoBarrels,
            "Thrust Ball" => BearingStyle::Thrust,
            _ => return None,
        })
    }

    /// Rows of rolling elements.
    pub fn rows(self) -> u32 {
        match self {
            BearingStyle::TwoBalls | BearingStyle::TwoBarrels => 2,
            _ => 1,
        }
    }
}

/// A filled shape in picture coordinates: a convex polygon or a circle.
enum Shape {
    Convex(Vec<(Real, Real)>),
    Circle((Real, Real), Real),
}

impl Shape {
    /// Signed distance, negative inside; for a polygon the largest over its sides.
    fn distance(&self, x: Real, y: Real) -> Real {
        match self {
            Shape::Circle((cx, cy), r) => (x - cx).hypot(y - cy) - r,
            Shape::Convex(p) => {
                // Which way the corners run decides which side is in.
                let n = p.len();
                let mut area = 0.0;
                for i in 0..n {
                    let (a, b) = (p[i], p[(i + 1) % n]);
                    area += a.0 * b.1 - b.0 * a.1;
                }
                let sign = if area >= 0.0 { 1.0 } else { -1.0 };
                let mut d = Real::NEG_INFINITY;
                for i in 0..n {
                    let (a, b) = (p[i], p[(i + 1) % n]);
                    let (ex, ey) = (b.0 - a.0, b.1 - a.1);
                    let len = ex.hypot(ey).max(1e-12);
                    // Outward of the side, for corners running counter-clockwise.
                    let side = sign * (ey * (x - a.0) - ex * (y - a.1)) / len;
                    d = d.max(side);
                }
                d
            }
        }
    }
}

/// One thing drawn; `None` for colour clears what is under it (a groove).
struct Layer {
    shape: Shape,
    fill: Option<[u8; 3]>,
}

const RING: [u8; 3] = [158, 165, 175];
const ELEMENT: [u8; 3] = [226, 230, 236];
const LINE: [u8; 3] = [52, 58, 68];
const AXIS: [u8; 3] = [120, 150, 200];

/// The layers of the section above the axis, in (along, radius) with the bearing `b` long and its
/// radii `r0`..`r1`.
fn upper(style: BearingStyle, r0: Real, r1: Real, b: Real) -> Vec<Layer> {
    let rect = |z0: Real, z1: Real, a: Real, c: Real| {
        Shape::Convex(vec![(z0, a), (z1, a), (z1, c), (z0, c)])
    };
    let solid = |shape: Shape, fill: [u8; 3]| Layer {
        shape,
        fill: Some(fill),
    };
    let clear = |shape: Shape| Layer { shape, fill: None };
    let t = r1 - r0;
    let mid = (r0 + r1) / 2.0;
    match style {
        BearingStyle::Ball | BearingStyle::TwoBalls => {
            let rows = style.rows();
            let rb = if rows == 2 {
                (t * 0.21).min(b * 0.2)
            } else {
                (t * 0.3).min(b * 0.42)
            };
            let wall = (t / 2.0 - 0.75 * rb).max(t * 0.12);
            let mut out = vec![
                solid(rect(0.0, b, r0, r0 + wall), RING),
                solid(rect(0.0, b, r1 - wall, r1), RING),
            ];
            for z in row_heights(rows, b) {
                out.push(clear(Shape::Circle((z, mid), rb * 1.08)));
                out.push(solid(Shape::Circle((z, mid), rb), ELEMENT));
            }
            out
        }
        BearingStyle::Roller | BearingStyle::Needle => {
            let wall = if style == BearingStyle::Needle {
                t * 0.18
            } else {
                t * 0.3
            };
            vec![
                solid(rect(0.0, b, r0, r0 + wall), RING),
                solid(rect(0.0, b, r1 - wall, r1), RING),
                solid(rect(b * 0.12, b * 0.88, r0 + wall, r1 - wall), ELEMENT),
            ]
        }
        BearingStyle::Tapered => {
            let (lean, wall) = (t * 0.16, t * 0.26);
            let slant = |z: Real, r: Real| (z, r + lean * (z / b - 0.5));
            let (p0, p1) = (slant(0.0, r0 + wall), slant(b, r0 + wall));
            let (q0, q1) = (slant(0.0, r1 - wall), slant(b, r1 - wall));
            vec![
                solid(Shape::Convex(vec![(0.0, r0), (b, r0), p1, p0]), RING),
                solid(Shape::Convex(vec![q0, q1, (b, r1), (0.0, r1)]), RING),
                solid(
                    Shape::Convex(vec![
                        (b * 0.12, p0.1 + (p1.1 - p0.1) * 0.12),
                        (b * 0.88, p0.1 + (p1.1 - p0.1) * 0.88),
                        (b * 0.88, q0.1 + (q1.1 - q0.1) * 0.88),
                        (b * 0.12, q0.1 + (q1.1 - q0.1) * 0.12),
                    ]),
                    ELEMENT,
                ),
            ]
        }
        BearingStyle::TwoBarrels => {
            let wall = t * 0.22;
            let (a, c) = (r0 + wall, r1 - wall);
            let half = b / 2.0;
            let (lean, gap) = ((c - a) * 0.22, b * 0.03);
            vec![
                solid(rect(0.0, b, r0, a), RING),
                solid(rect(0.0, b, c, r1), RING),
                // Two rows leaning apart, as the outer race's sphere makes them.
                solid(
                    Shape::Convex(vec![
                        (b * 0.08, a),
                        (half - gap, a),
                        (half - gap, c - lean),
                        (b * 0.08, c),
                    ]),
                    ELEMENT,
                ),
                solid(
                    Shape::Convex(vec![
                        (half + gap, a),
                        (b * 0.92, a),
                        (b * 0.92, c),
                        (half + gap, c - lean),
                    ]),
                    ELEMENT,
                ),
            ]
        }
        BearingStyle::Thrust => {
            let washer = b * 0.3;
            vec![
                solid(rect(0.0, washer, r0, r1), RING),
                solid(rect(b - washer, b, r0, r1), RING),
                solid(
                    Shape::Circle((b / 2.0, mid), (b / 2.0 - washer).min(t * 0.4) * 0.98),
                    ELEMENT,
                ),
            ]
        }
    }
}

/// Where the rows stand along the axis.
pub fn row_heights(rows: u32, b: Real) -> Vec<Real> {
    if rows == 2 {
        vec![b * 0.28, b * 0.72]
    } else {
        vec![b / 2.0]
    }
}

/// The section above the axis as a square RGBA picture (`size` pixels, 4x supersampled, clear
/// background), filling it: the list shows pictures small, and both sides of the axis came out too
/// tiny to read. Proportions are kept within what stays legible; the axis is dashed below it.
pub fn bearing_section(style: BearingStyle, d: Real, od: Real, b: Real, size: u32) -> Vec<u8> {
    let t_real = (od - d) / 2.0;
    let (r0, r1) = (t_real, 2.0 * t_real);
    let b = b.clamp(t_real * 0.7, t_real * 1.5);
    let n = size as usize;
    let mut out = vec![0u8; n * n * 4];
    let margin = 2.0;
    let axis_room = Real::from(size) * 0.14;
    let scale = (Real::from(size) - 2.0 * margin - axis_room) / t_real.max(b);
    let centre = Real::from(size) / 2.0;
    let bottom = centre + t_real * scale / 2.0 - axis_room / 2.0 + margin / 2.0;
    let axis_y = Real::from(size) - margin - 0.5;
    let layers = upper(style, r0, r1, b);
    // Two pixels of the picture: it is shown at half size.
    let line = 2.0 / scale;
    let samples = 4;
    for py in 0..n {
        for px in 0..n {
            let mut sum = [0.0f64; 4];
            for sy in 0..samples {
                for sx in 0..samples {
                    let x = px as Real + (Real::from(sx) + 0.5) / Real::from(samples);
                    let y = py as Real + (Real::from(sy) + 0.5) / Real::from(samples);
                    let z = (x - centre) / scale + b / 2.0;
                    let r = r0 + (bottom - y) / scale;
                    let mut colour: Option<[u8; 3]> = None;
                    for l in &layers {
                        let dist = l.shape.distance(z, r);
                        if dist <= 0.0 {
                            colour = l.fill.map(|f| if dist > -line { LINE } else { f });
                        }
                    }
                    if colour.is_none()
                        && (y - axis_y).abs() < 1.0
                        && ((x / 6.0).floor() as i64).rem_euclid(2) == 0
                    {
                        colour = Some(AXIS);
                    }
                    if let Some(c) = colour {
                        sum[0] += Real::from(c[0]);
                        sum[1] += Real::from(c[1]);
                        sum[2] += Real::from(c[2]);
                        sum[3] += 1.0;
                    }
                }
            }
            if sum[3] > 0.0 {
                let at = (py * n + px) * 4;
                if let Some(p) = out.get_mut(at..at + 4) {
                    p[0] = (sum[0] / sum[3]) as u8;
                    p[1] = (sum[1] / sum[3]) as u8;
                    p[2] = (sum[2] / sum[3]) as u8;
                    p[3] = (255.0 * sum[3] / Real::from(samples * samples)) as u8;
                }
            }
        }
    }
    out
}
