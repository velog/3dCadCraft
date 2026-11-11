//! The shaft generator's calculation (spec 6.2-design-calculators §3.2).
//!
//! The beam: two supports, radial forces in two planes, distributed loads, the
//! shaft's own weight, axial forces and torques; shear force, bending moment,
//! slope and deflection by integrating M / (E·I) over the divisions (Euler–
//! Bernoulli beam, I of the local diameter), the supports fixing the two
//! constants. Fatigue, twist and critical speed after S. H. Loewenthal, *Design
//! of Power-Transmitting Shafts*, NASA Reference Publication 1123, 1984 (public
//! domain); equation, table and figure numbers below are that report's.
//!
//! Units: N, mm, MPa, N·mm; density kg/m³.

use std::f64::consts::PI;

use serde::{Deserialize, Serialize};

use super::G_MM_S2;
use super::shaft::ShaftDesign;

/// The shaft's material.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Material {
    /// Its name.
    pub name: String,
    /// Ultimate tensile strength σu.
    pub ultimate: f64,
    /// Tensile yield strength σy.
    pub yield_strength: f64,
    /// Bending fatigue limit of a polished, unnotched specimen at 10⁶ cycles σ'f.
    pub fatigue_limit: f64,
    /// Young's modulus E.
    pub e: f64,
    /// Shear modulus G.
    pub g: f64,
    /// Density, kg/m³.
    pub density: f64,
    /// Through hardened above 200 BHN (the keyway factors of Table V).
    pub hardened: bool,
}

impl Material {
    /// SAE 1045 quenched and tempered, 225 BHN (RP-1123 Table II).
    pub fn sae_1045() -> Material {
        Material {
            name: "SAE 1045, quenched and tempered (225 BHN)".into(),
            ultimate: 724.0,
            yield_strength: 634.0,
            fatigue_limit: 323.0,
            e: 206_000.0,
            g: 79_300.0,
            density: 7850.0,
            hardened: true,
        }
    }

    /// SAE 4140 quenched and tempered, 310 BHN (Table II).
    pub fn sae_4140() -> Material {
        Material {
            name: "SAE 4140, quenched and tempered (310 BHN)".into(),
            ultimate: 1076.0,
            yield_strength: 965.0,
            fatigue_limit: 619.0,
            ..Material::sae_1045()
        }
    }

    /// SAE 1020 hot-rolled, 108 BHN (Table II).
    pub fn sae_1020() -> Material {
        Material {
            name: "SAE 1020, hot rolled (108 BHN)".into(),
            ultimate: 441.0,
            yield_strength: 262.0,
            fatigue_limit: 208.0,
            hardened: false,
            ..Material::sae_1045()
        }
    }

    /// The presets.
    pub fn presets() -> Vec<Material> {
        vec![
            Material::sae_1045(),
            Material::sae_4140(),
            Material::sae_1020(),
        ]
    }

    /// Fatigue strength coefficient σ'f at one cycle, eq. 27: σu + 345 MPa.
    pub fn strength_coefficient(&self) -> f64 {
        self.ultimate + 345.0
    }
}

/// A support.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SupportKind {
    /// Takes radial and axial load.
    #[default]
    Fixed,
    /// Takes radial load only (slides along the axis).
    Free,
}

/// A support at `z` from the shaft's start.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Support {
    /// Where.
    pub z: f64,
    /// Which kind.
    pub kind: SupportKind,
}

/// A load. Radial directions are angles about the axis from +Y (0: along +Y,
/// 90°: along +X); gravity acts along −Y.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Load {
    /// A radial force (N) at `z`.
    Radial {
        /// Where.
        z: f64,
        /// Its size.
        force: f64,
        /// Its direction.
        angle: f64,
    },
    /// An axial force (N, + along +Z) at `z`.
    Axial {
        /// Where.
        z: f64,
        /// Its size.
        force: f64,
    },
    /// A distributed radial load (N/mm) from `z0` to `z1`.
    Continuous {
        /// Start.
        z0: f64,
        /// End.
        z1: f64,
        /// Load per length.
        q: f64,
        /// Its direction.
        angle: f64,
    },
    /// A torque (N·mm) applied at `z`; all torques must sum to zero.
    Torque {
        /// Where.
        z: f64,
        /// Its size, about +Z.
        torque: f64,
    },
    /// A bending moment (N·mm) applied at `z` (a gear's axial force acting at
    /// its pitch radius, a coupling's misalignment). It bends the shaft in the
    /// plane of `angle` (the plane a radial force at that angle bends it in);
    /// positive: the internal moment steps up by it.
    Moment {
        /// Where.
        z: f64,
        /// Its size.
        moment: f64,
        /// The plane it bends in.
        angle: f64,
    },
}

/// How the reduced stress combines bending and torsion.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ReducedStress {
    /// Distortion energy: √(σ² + 3τ²).
    #[default]
    Hmh,
    /// Maximum shear: √(σ² + 4τ²).
    Tresca,
}

/// Reliability levels of Table III.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Reliability {
    /// 50 %.
    R50,
    /// 90 %.
    #[default]
    R90,
    /// 95 %.
    R95,
    /// 99 %.
    R99,
    /// 99.9 %.
    R999,
    /// 99.99 %.
    R9999,
}

impl Reliability {
    /// All levels, in order.
    pub const ALL: [Reliability; 6] = [
        Reliability::R50,
        Reliability::R90,
        Reliability::R95,
        Reliability::R99,
        Reliability::R999,
        Reliability::R9999,
    ];

    /// The standard normal variate of the level.
    fn z(self) -> f64 {
        match self {
            Reliability::R50 => 0.0,
            Reliability::R90 => 1.2816,
            Reliability::R95 => 1.6449,
            Reliability::R99 => 2.3263,
            Reliability::R999 => 3.0902,
            Reliability::R9999 => 3.7190,
        }
    }

    /// The reliability factor kc of Table III for a standard deviation of 8 %
    /// of the mean fatigue limit (the report's recommendation): 1 − 0.08·z.
    pub fn factor(self) -> f64 {
        1.0 - 0.08 * self.z()
    }

    /// The level as text.
    pub fn label(self) -> &'static str {
        match self {
            Reliability::R50 => "50 %",
            Reliability::R90 => "90 %",
            Reliability::R95 => "95 %",
            Reliability::R99 => "99 %",
            Reliability::R999 => "99.9 %",
            Reliability::R9999 => "99.99 %",
        }
    }
}

/// The fatigue-life modifying factors of eq. 19 the user gives.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FatigueFactors {
    /// Surface factor ka, read off Fig. 3 for the finish and σu (machined
    /// ≈ 0.7 … 0.8 for usual steels).
    pub surface: f64,
    /// Reliability (kc, Table III).
    pub reliability: Reliability,
    /// Temperature factor kd (Table IV: 1 between −73 and 316 °C).
    pub temperature: f64,
    /// The product of the other factors kf·kg·kh·ki (press fit, residual
    /// stress, corrosion, other).
    pub other: f64,
    /// Theoretical stress concentration in bending (Kt)b at the worst notch
    /// (1: none). Keyways take Table V instead.
    pub kt_bending: f64,
    /// Notch sensitivity q (Fig. 5).
    pub notch_sensitivity: f64,
    /// Required factor of safety FS.
    pub safety: f64,
}

impl Default for FatigueFactors {
    fn default() -> FatigueFactors {
        FatigueFactors {
            surface: 0.75,
            reliability: Reliability::R90,
            temperature: 1.0,
            other: 1.0,
            kt_bending: 1.0,
            notch_sensitivity: 0.8,
            safety: 2.0,
        }
    }
}

/// Size factor kb: eq. 21 up to 50 mm (Kuguel), eq. 22 from 50 to 250 mm.
/// The report prints eq. 21's metric form with "millimeters" but its Fig. 4
/// labels the same constant 0.76 as centimetres, and only that agrees with the
/// inch form (d / 0.3 in); we use d / 7.62 mm.
pub fn size_factor(d: f64) -> f64 {
    if d <= 50.0 {
        (d / 7.62).powf(-0.068)
    } else {
        1.85 * d.powf(-0.19)
    }
}

/// Fatigue stress concentration factor ke, eq. 25: 1 / (1 + q (Kt − 1)).
pub fn notch_factor(kt: f64, q: f64) -> f64 {
    1.0 / (1.0 + q * (kt - 1.0))
}

/// Table V: the keyway factors (ke)b, (ke)t for a profiled keyway.
pub fn keyway_factors(hardened: bool) -> (f64, f64) {
    if hardened { (0.50, 0.63) } else { (0.63, 0.77) }
}

/// Corrected bending fatigue limit of the shaft, eq. 19.
pub fn corrected_limit(m: &Material, f: &FatigueFactors, d: f64, ke: f64) -> f64 {
    f.surface
        * size_factor(d)
        * f.reliability.factor()
        * f.temperature
        * ke
        * f.other
        * m.fatigue_limit
}

/// Factor of safety of a solid (B = 1) or hollow section by eq. 17 (reversed
/// bending Mr, steady torque Tm): FS = π d³ / (32 B √((Mr/σf)² + ¾ (Tm/σy)²)).
pub fn safety_eq17(d: f64, b: f64, mr: f64, tm: f64, sf: f64, sy: f64) -> f64 {
    let r = ((mr / sf).powi(2) + 0.75 * (tm / sy).powi(2)).sqrt();
    if r <= 0.0 {
        return f64::INFINITY;
    }
    PI * d.powi(3) / (32.0 * b * r)
}

/// Diameter by eq. 17.
pub fn diameter_eq17(fs: f64, b: f64, mr: f64, tm: f64, sf: f64, sy: f64) -> f64 {
    (32.0 * fs / PI * b * ((mr / sf).powi(2) + 0.75 * (tm / sy).powi(2)).sqrt()).cbrt()
}

/// Specimen fatigue limit with a mean torque `tm` and mean moment `mm` on a
/// shaft of diameter `d`, eq. 28.
pub fn limit_with_mean(sf0: f64, tm: f64, mm: f64, d: f64, sy: f64, su: f64) -> f64 {
    let d3 = d.powi(3);
    let a = (1.0 - 77.8 * (tm / (d3 * sy)).powi(2)).max(0.0).sqrt();
    sf0 * (a - 10.2 * mm / (d3 * su))
}

/// Slope of the S-N line on log-log axes (§ Variable-Amplitude Loading):
/// b = log(σ'f / σf) / log(Nf), Nf = 10⁶ → divide by 6.
pub fn sn_slope(coefficient: f64, sf: f64, nf: f64) -> f64 {
    (coefficient / sf).log10() / nf.log10()
}

/// Diameter for blocks of reversed moments by Miner's rule, eq. 31: `blocks`
/// as (n_i / N_f, M_i).
pub fn diameter_eq31(fs: f64, sf: f64, b: f64, blocks: &[(f64, f64)]) -> f64 {
    let s: f64 = blocks.iter().map(|&(r, m)| r * m.powf(1.0 / b)).sum();
    (32.0 * fs / (PI * sf) * s.powf(b)).cbrt()
}

/// Angle of twist in degrees of a length `l` of diameter `d` under torque `t`,
/// eq. 32 (θ = 584 T L / (d⁴ G), the 584 being 32 / π · 180 / π).
pub fn twist_degrees(t: f64, l: f64, d: f64, g: f64) -> f64 {
    32.0 / PI * 180.0 / PI * t * l / (d.powi(4) * g)
}

/// The calculation's inputs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShaftLoads {
    /// The material.
    pub material: Material,
    /// The supports (exactly two).
    pub supports: Vec<Support>,
    /// The loads.
    pub loads: Vec<Load>,
    /// Include the shaft's own weight.
    pub use_density: bool,
    /// Number of divisions along the shaft.
    pub divisions: usize,
    /// Reduced stress mode.
    pub reduced: ReducedStress,
    /// Fatigue factors.
    pub fatigue: FatigueFactors,
}

impl Default for ShaftLoads {
    fn default() -> ShaftLoads {
        ShaftLoads {
            material: Material::sae_1045(),
            supports: Vec::new(),
            loads: Vec::new(),
            use_density: true,
            divisions: 400,
            reduced: ReducedStress::Hmh,
            fatigue: FatigueFactors::default(),
        }
    }
}

/// The results along the shaft, one value per node.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShaftResults {
    /// Node positions.
    pub z: Vec<f64>,
    /// Diameter used.
    pub d: Vec<f64>,
    /// Shear force (resultant).
    pub shear: Vec<f64>,
    /// Bending moment (resultant).
    pub moment: Vec<f64>,
    /// Torque.
    pub torque: Vec<f64>,
    /// Axial force (+ tension).
    pub axial: Vec<f64>,
    /// Deflection (resultant).
    pub deflection: Vec<f64>,
    /// Bending stress.
    pub bending_stress: Vec<f64>,
    /// Torsional stress.
    pub torsion_stress: Vec<f64>,
    /// Reduced stress.
    pub reduced_stress: Vec<f64>,
    /// Fatigue factor of safety by eq. 17.
    pub safety: Vec<f64>,
    /// Support reactions (Y, X, Z) in the order of the supports.
    pub reactions: Vec<[f64; 3]>,
    /// Mass, kg.
    pub mass: f64,
    /// Angle of twist between the torques, degrees.
    pub twist: f64,
    /// First critical speed by eq. 33, rpm (none without weight).
    pub critical_rpm: Option<f64>,
}

impl ShaftResults {
    /// The largest absolute value of a series.
    pub fn max_abs(v: &[f64]) -> f64 {
        v.iter().fold(0.0, |a: f64, &b| a.max(b.abs()))
    }

    /// The smallest safety factor and where.
    pub fn min_safety(&self) -> (f64, f64) {
        self.safety.iter().zip(&self.z).fold(
            (f64::INFINITY, 0.0),
            |a, (&s, &z)| if s < a.0 { (s, z) } else { a },
        )
    }
}

/// A point force (or, in the moment list, a point moment `f`) in one plane.
#[derive(Clone, Copy)]
struct Point {
    z: f64,
    f: f64,
}

/// Beam in one plane on two supports: their reactions, then shear and moment
/// at the nodes. The internal moment at z is the moment of everything left
/// of z: Σ f (z − zi) + Σ C.
fn plane(
    points: &[Point],
    moments: &[Point],
    za: f64,
    zb: f64,
    nodes: &[f64],
) -> (f64, f64, Vec<f64>, Vec<f64>) {
    let sum: f64 = points.iter().map(|p| p.f).sum();
    let moment_a: f64 = points.iter().map(|p| p.f * (p.z - za)).sum();
    let couples: f64 = moments.iter().map(|c| c.f).sum();
    // Nothing is left past the right end: Σ f = 0 and Σ f (L − zi) + Σ C = 0.
    let rb = (couples - moment_a) / (zb - za);
    let ra = -sum - rb;
    let mut all: Vec<Point> = points.to_vec();
    all.push(Point { z: za, f: ra });
    all.push(Point { z: zb, f: rb });
    let mut v = Vec::with_capacity(nodes.len());
    let mut m = Vec::with_capacity(nodes.len());
    for &z in nodes {
        let (mut vz, mut mz) = (0.0, 0.0);
        for p in all.iter().filter(|p| p.z < z - 1e-12) {
            vz += p.f;
            mz += p.f * (z - p.z);
        }
        for c in moments.iter().filter(|c| c.z < z - 1e-12) {
            mz += c.f;
        }
        v.push(vz);
        m.push(mz);
    }
    (ra, rb, v, m)
}

/// One plane of the beam on any number of supports (`sup`, at least two
/// apart): each support's reaction, shear, moment and deflection at the nodes.
///
/// Statically indeterminate for three or more: the flexibility method — the
/// outermost two supports carry the loads, the others are unknown forces
/// whose deflections on that base must vanish (one linear equation per inner
/// support, the deflections from the same M / (E I) integration).
struct PlaneResult {
    reactions: Vec<f64>,
    shear: Vec<f64>,
    moment: Vec<f64>,
    deflection: Vec<f64>,
}

fn plane_n(
    points: &[Point],
    moments: &[Point],
    sup: &[f64],
    nodes: &[f64],
    ei: &[f64],
) -> Result<PlaneResult, String> {
    let ia = (0..sup.len())
        .min_by(|&a, &b| sup[a].total_cmp(&sup[b]))
        .ok_or("no supports")?;
    let ib = (0..sup.len())
        .max_by(|&a, &b| sup[a].total_cmp(&sup[b]))
        .ok_or("no supports")?;
    let (za, zb) = (sup[ia], sup[ib]);
    let inner: Vec<usize> = (0..sup.len()).filter(|&k| k != ia && k != ib).collect();
    let base = |pts: &[Point], cs: &[Point]| {
        let (ra, rb, v, m) = plane(pts, cs, za, zb, nodes);
        let y = deflection(nodes, &m, ei, za, zb);
        (ra, rb, v, m, y)
    };
    let mut forces = vec![0.0; inner.len()];
    if !inner.is_empty() {
        let (_, _, _, _, y0) = base(points, moments);
        let n = inner.len();
        // a[j][k]: deflection at inner support j from a unit force at k.
        let mut a = vec![vec![0.0; n + 1]; n];
        for (k, &sk) in inner.iter().enumerate() {
            let (_, _, _, _, yk) = base(&[Point { z: sup[sk], f: 1.0 }], &[]);
            for (j, &sj) in inner.iter().enumerate() {
                a[j][k] = interp(nodes, &yk, sup[sj]);
            }
        }
        for (j, &sj) in inner.iter().enumerate() {
            a[j][n] = -interp(nodes, &y0, sup[sj]);
        }
        forces = solve_linear(a).ok_or("two supports stand at the same place")?;
    }
    let mut all = points.to_vec();
    for (k, &sk) in inner.iter().enumerate() {
        all.push(Point {
            z: sup[sk],
            f: forces[k],
        });
    }
    let (ra, rb, v, m, y) = base(&all, moments);
    let mut reactions = vec![0.0; sup.len()];
    reactions[ia] = ra;
    reactions[ib] = rb;
    for (k, &sk) in inner.iter().enumerate() {
        reactions[sk] = forces[k];
    }
    Ok(PlaneResult {
        reactions,
        shear: v,
        moment: m,
        deflection: y,
    })
}

/// The value of `y` (given at `nodes`) at `z`, linear between nodes.
fn interp(nodes: &[f64], y: &[f64], z: f64) -> f64 {
    let n = nodes.len();
    if n < 2 {
        return y.first().copied().unwrap_or(0.0);
    }
    let k = nodes.partition_point(|&x| x < z).clamp(1, n - 1);
    let (x0, x1) = (nodes[k - 1], nodes[k]);
    let t = if x1 > x0 { (z - x0) / (x1 - x0) } else { 0.0 };
    y[k - 1] + t * (y[k] - y[k - 1])
}

/// Gaussian elimination with partial pivoting on the augmented matrix `a`
/// (n rows of n + 1); `None` when singular.
fn solve_linear(mut a: Vec<Vec<f64>>) -> Option<Vec<f64>> {
    let n = a.len();
    let scale = a
        .iter()
        .flat_map(|r| r[..n].iter())
        .fold(0.0f64, |m, v| m.max(v.abs()));
    for c in 0..n {
        let p = (c..n).max_by(|&i, &j| a[i][c].abs().total_cmp(&a[j][c].abs()))?;
        if a[p][c].abs() <= 1e-12 * scale {
            return None;
        }
        a.swap(c, p);
        let pivot = a[c].clone();
        for (r, row) in a.iter_mut().enumerate() {
            if r != c {
                let f = row[c] / pivot[c];
                for (x, p) in row.iter_mut().zip(&pivot).skip(c) {
                    *x -= f * p;
                }
            }
        }
    }
    Some((0..n).map(|i| a[i][n] / a[i][i]).collect())
}

/// Deflection from the moment: two integrations of M / (E I) by trapezoids,
/// then the straight line through zero at both supports.
fn deflection(nodes: &[f64], m: &[f64], ei: &[f64], za: f64, zb: f64) -> Vec<f64> {
    let n = nodes.len();
    let mut slope = vec![0.0; n];
    let mut y = vec![0.0; n];
    for i in 1..n {
        let h = nodes[i] - nodes[i - 1];
        slope[i] = slope[i - 1] + h * (m[i] / ei[i] + m[i - 1] / ei[i - 1]) / 2.0;
        y[i] = y[i - 1] + h * (slope[i] + slope[i - 1]) / 2.0;
    }
    let (ya, yb) = (interp(nodes, &y, za), interp(nodes, &y, zb));
    let c1 = (yb - ya) / (zb - za);
    nodes
        .iter()
        .zip(&y)
        .map(|(&z, &v)| v - ya - c1 * (z - za))
        .collect()
}

/// Runs the calculation.
pub fn calculate(shaft: &ShaftDesign, loads: &ShaftLoads) -> Result<ShaftResults, String> {
    shaft.validate()?;
    let len = shaft.length();
    if loads.supports.len() < 2 {
        return Err("the calculation needs at least two supports".into());
    }
    let sup: Vec<f64> = loads.supports.iter().map(|s| s.z).collect();
    for &z in &sup {
        if !(0.0..=len).contains(&z) {
            return Err(format!("the support at z = {z} lies outside the shaft"));
        }
    }
    for (i, &a) in sup.iter().enumerate() {
        if sup[i + 1..].iter().any(|&b| (a - b).abs() < 1e-6) {
            return Err("two supports stand at the same place".into());
        }
    }
    let n = loads.divisions.max(10);
    // Even divisions, plus a node at every support and point load (the
    // reactions and the inner supports' zero deflection are read there).
    let mut nodes: Vec<f64> = (0..=n).map(|i| len * i as f64 / n as f64).collect();
    let mut extra = sup.clone();
    for l in &loads.loads {
        match *l {
            Load::Radial { z, .. } | Load::Moment { z, .. } => extra.push(z),
            _ => {}
        }
    }
    for z in extra {
        let z = z.clamp(0.0, len);
        if nodes.iter().all(|&x| (x - z).abs() > 1e-9 * len.max(1.0)) {
            nodes.push(z);
        }
    }
    nodes.sort_by(f64::total_cmp);
    let n = nodes.len() - 1;
    let d: Vec<f64> = nodes.iter().map(|&z| shaft.diameter_at(z)).collect();
    // A bore makes the section hollow (RP-1123 eq. 4: B = 1 / (1 - (di/do)^4)).
    let di: Vec<f64> = nodes.iter().map(|&z| shaft.inner_at(z)).collect();
    let mat = &loads.material;
    let ei: Vec<f64> = d
        .iter()
        .zip(&di)
        .map(|(&d, &i)| mat.e * PI * (d.powi(4) - i.powi(4)) / 64.0)
        .collect();
    // Point forces and moments per plane: Y (angle 0) and X (90°).
    let (mut py, mut px) = (Vec::new(), Vec::new());
    let (mut cy, mut cx) = (Vec::new(), Vec::new());
    let mut torques = Vec::new();
    let mut axials = Vec::new();
    for l in &loads.loads {
        match *l {
            Load::Moment { z, moment, angle } => {
                check_on(z, len)?;
                cy.push(Point {
                    z,
                    f: moment * angle.cos(),
                });
                cx.push(Point {
                    z,
                    f: moment * angle.sin(),
                });
            }
            Load::Radial { z, force, angle } => {
                check_on(z, len)?;
                py.push(Point {
                    z,
                    f: force * angle.cos(),
                });
                px.push(Point {
                    z,
                    f: force * angle.sin(),
                });
            }
            Load::Continuous { z0, z1, q, angle } => {
                check_on(z0, len)?;
                check_on(z1, len)?;
                let (a, b) = (z0.min(z1), z0.max(z1));
                let k = ((b - a) / len * n as f64).ceil().max(1.0) as usize;
                let h = (b - a) / k as f64;
                for i in 0..k {
                    let z = a + h * (i as f64 + 0.5);
                    py.push(Point {
                        z,
                        f: q * h * angle.cos(),
                    });
                    px.push(Point {
                        z,
                        f: q * h * angle.sin(),
                    });
                }
            }
            Load::Axial { z, force } => {
                check_on(z, len)?;
                axials.push((z, force));
            }
            Load::Torque { z, torque } => {
                check_on(z, len)?;
                torques.push((z, torque));
            }
        }
    }
    let tsum: f64 = torques.iter().map(|t| t.1).sum();
    let tmax = torques.iter().fold(0.0, |a: f64, t| a.max(t.1.abs()));
    if tsum.abs() > 1e-6 * tmax.max(1.0) {
        return Err(format!(
            "the torques must sum to zero (they sum to {:.1} N·m)",
            tsum / 1000.0
        ));
    }
    // Own weight: each division's weight at its middle, along −Y.
    let mut weights = Vec::new();
    let mut mass = 0.0;
    for i in 0..n {
        let (a, b) = (nodes[i], nodes[i + 1]);
        let (dm, im) = (
            shaft.diameter_at((a + b) / 2.0),
            shaft.inner_at((a + b) / 2.0),
        );
        let kg = mat.density * 1e-9 * PI / 4.0 * (dm * dm - im * im) * (b - a);
        mass += kg;
        weights.push(Point {
            z: (a + b) / 2.0,
            f: -kg * G_MM_S2 / 1000.0,
        });
    }
    if loads.use_density {
        py.extend(weights.iter().map(|w| Point { z: w.z, f: w.f }));
    }
    let pyr = plane_n(&py, &cy, &sup, &nodes, &ei)?;
    let pxr = plane_n(&px, &cx, &sup, &nodes, &ei)?;
    let (vy, my, yy) = (&pyr.shear, &pyr.moment, &pyr.deflection);
    let (vx, mx, yx) = (&pxr.shear, &pxr.moment, &pxr.deflection);
    // Torque and axial force at each node (from the start).
    let torque: Vec<f64> = nodes
        .iter()
        .map(|&z| torques.iter().filter(|t| t.0 < z).map(|t| t.1).sum())
        .collect();
    // The axial reaction is at the fixed support (the first one when both are).
    let fixed = loads
        .supports
        .iter()
        .find(|s| s.kind == SupportKind::Fixed)
        .map(|s| s.z);
    let fsum: f64 = axials.iter().map(|a| a.1).sum();
    if fixed.is_none() && fsum.abs() > 1e-9 {
        return Err("an axial force needs a fixed support".into());
    }
    let mut ax_all = axials.clone();
    if let Some(zf) = fixed {
        ax_all.push((zf, -fsum));
    }
    // Internal force at z: minus the sum of the forces before it (tension +).
    let axial: Vec<f64> = nodes
        .iter()
        .map(|&z| -ax_all.iter().filter(|a| a.0 < z).map(|a| a.1).sum::<f64>())
        .collect();
    let mut r = ShaftResults {
        z: nodes.clone(),
        d: d.clone(),
        mass,
        ..ShaftResults::default()
    };
    let ff = &loads.fatigue;
    for i in 0..=n {
        let (dd, b) = (d[i], 1.0 / (1.0 - (di[i] / d[i]).powi(4)));
        let m = my[i].hypot(mx[i]);
        let t = torque[i];
        let sb = 32.0 * b * m / (PI * dd.powi(3));
        let st = 16.0 * b * t.abs() / (PI * dd.powi(3));
        let sa = axial[i] / (PI * (dd * dd - di[i] * di[i]) / 4.0);
        let s = sb + sa.abs();
        let red = match loads.reduced {
            ReducedStress::Hmh => (s * s + 3.0 * st * st).sqrt(),
            ReducedStress::Tresca => (s * s + 4.0 * st * st).sqrt(),
        };
        let keyed = shaft
            .section_at(nodes[i])
            .is_some_and(|(k, _)| shaft.sections[k].keyway.is_some());
        let ke = if keyed {
            keyway_factors(mat.hardened).0
        } else {
            notch_factor(ff.kt_bending, ff.notch_sensitivity)
        };
        let sf = corrected_limit(mat, ff, dd, ke);
        r.shear.push(vy[i].hypot(vx[i]));
        r.moment.push(m);
        r.torque.push(t);
        r.axial.push(axial[i]);
        r.deflection.push(yy[i].hypot(yx[i]));
        r.bending_stress.push(sb);
        r.torsion_stress.push(st);
        r.reduced_stress.push(red);
        r.safety
            .push(safety_eq17(dd, b, m, t.abs(), sf, mat.yield_strength));
    }
    let axial_at = |z: f64| if Some(z) == fixed { -fsum } else { 0.0 };
    r.reactions = loads
        .supports
        .iter()
        .enumerate()
        .map(|(k, s)| [pyr.reactions[k], pxr.reactions[k], axial_at(s.z)])
        .collect();
    // Twist: θ summed over the divisions (eq. 32 per piece).
    r.twist = (0..n)
        .map(|i| {
            let t = (torque[i] + torque[i + 1]) / 2.0;
            let zm = (nodes[i] + nodes[i + 1]) / 2.0;
            // A hollow piece: d⁴ becomes do⁴ - di⁴.
            let dm = (shaft.diameter_at(zm).powi(4) - shaft.inner_at(zm).powi(4)).powf(0.25);
            twist_degrees(t.abs(), nodes[i + 1] - nodes[i], dm, mat.g)
        })
        .sum();
    // Eq. 33: Rayleigh–Ritz from the static deflection under the own weight.
    let yw = plane_n(&weights, &[], &sup, &nodes, &ei)?.deflection;
    let (mut num, mut den) = (0.0, 0.0);
    for (i, w) in weights.iter().enumerate() {
        let y = (yw[i] + yw[i + 1]) / 2.0;
        num += w.f.abs() * y.abs();
        den += w.f.abs() * y * y;
    }
    r.critical_rpm = (den > 0.0).then(|| (G_MM_S2 * num / den).sqrt() * 60.0 / (2.0 * PI));
    Ok(r)
}

fn check_on(z: f64, len: f64) -> Result<(), String> {
    if (-1e-9..=len + 1e-9).contains(&z) {
        Ok(())
    } else {
        Err(format!("the load at z = {z} lies outside the shaft"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::shaft::Section;

    fn close(a: f64, b: f64, rel: f64) -> bool {
        (a - b).abs() <= rel * b.abs()
    }

    /// RP-1123, Application Example: SAE 1045 (σu 724, σy 634, σ'f 323, σ'f
    /// coefficient 1227 MPa), 3000 N·m steady torque, FS 2, 100 000 cycles of
    /// 2000 / 1500 / 1000 N·m (15 / 35 / 50 %), all k factors 0.4.
    #[test]
    fn application_example_of_rp1123() {
        let m = Material::sae_1045();
        // Eq. 28 at the first guess d = 55 mm: 313 MPa.
        let sfm = limit_with_mean(
            m.fatigue_limit,
            3000e3,
            0.0,
            55.0,
            m.yield_strength,
            m.ultimate,
        );
        assert!(close(sfm, 313.0, 0.002), "{sfm}");
        let sf = 0.4 * 313.0;
        // b = log(1227 / 125) / 6 = 0.165, 1/b = 6.05.
        let b = sn_slope(1227.0, 125.0, 1e6);
        assert!(close(b, 0.165, 0.003) && close(1.0 / b, 6.05, 0.003), "{b}");
        // d³ = 1.71e-4 m³, d = 0.056 m. The report raises to 6.05 and then to
        // 0.165 (not quite inverse) in N·m and Pa; with one b the result is 1.4 % above.
        let blocks = [(0.015, 2000e3), (0.035, 1500e3), (0.05, 1000e3)];
        let d = diameter_eq31(2.0, 125.0, 0.165, &blocks);
        assert!(close(d.powi(3), 1.71e5, 0.015), "{}", d.powi(3));
        assert!(close(d, 56.0, 0.01), "{d}");
        // Only the 2000 N·m block: d = 0.054 m.
        let d1 = diameter_eq31(2.0, sf, b, &blocks[..1]);
        assert!(close(d1, 54.0, 0.015), "{d1}");
    }

    #[test]
    fn factors_of_eq19() {
        // Table III, 8 % column.
        let want = [1.0, 0.897, 0.868, 0.814, 0.753, 0.702];
        for (r, w) in Reliability::ALL.iter().zip(want) {
            assert!(
                (r.factor() - w).abs() < 0.0015,
                "{} {}",
                r.label(),
                r.factor()
            );
        }
        // Eq. 21 and 22 meet at 50 mm; 7.62 mm is the specimen (1).
        assert!(close(size_factor(7.62), 1.0, 1e-12));
        assert!(close(size_factor(50.0), 1.85 * 50f64.powf(-0.19), 0.002));
        // Eq. 25 with the report's Fig. 8 notch: Kt 1.76, q 0.79 → ke ≈ 0.63.
        assert!(close(notch_factor(1.76, 0.79), 0.625, 0.005));
        // Eq. 32: 584 T L / (d⁴ G).
        let t = twist_degrees(1e6, 1000.0, 50.0, 79_300.0);
        assert!(close(
            t,
            584.0 * 1e6 * 1000.0 / (50f64.powi(4) * 79_300.0),
            0.001
        ));
        // Eq. 17 and its inverse agree.
        let d = diameter_eq17(2.0, 1.0, 500e3, 800e3, 150.0, 600.0);
        assert!(close(
            safety_eq17(d, 1.0, 500e3, 800e3, 150.0, 600.0),
            2.0,
            1e-9
        ));
    }

    fn plain(d: f64, l: f64) -> ShaftDesign {
        ShaftDesign {
            sections: vec![Section::cylinder(d, l)],
            ..ShaftDesign::default()
        }
    }

    /// A simply supported beam with a central force: R = F/2, M = F L / 4,
    /// deflection F L³ / (48 E I).
    #[test]
    fn central_force_on_two_supports() {
        let s = plain(40.0, 1000.0);
        let loads = ShaftLoads {
            supports: vec![
                Support {
                    z: 0.0,
                    kind: SupportKind::Fixed,
                },
                Support {
                    z: 1000.0,
                    kind: SupportKind::Free,
                },
            ],
            loads: vec![Load::Radial {
                z: 500.0,
                force: 10_000.0,
                angle: 0.0,
            }],
            use_density: false,
            ..ShaftLoads::default()
        };
        let r = calculate(&s, &loads).unwrap();
        assert!(close(r.reactions[0][0], -5000.0, 1e-9));
        assert!(close(ShaftResults::max_abs(&r.moment), 2.5e6, 1e-6));
        let i = PI * 40f64.powi(4) / 64.0;
        let want = 10_000.0 * 1e9 / (48.0 * 206_000.0 * i);
        assert!(
            close(ShaftResults::max_abs(&r.deflection), want, 1e-3),
            "{want}"
        );
        let sb = 32.0 * 2.5e6 / (PI * 64_000.0);
        assert!(close(ShaftResults::max_abs(&r.bending_stress), sb, 1e-6));
        assert!(r.critical_rpm.is_some());
    }

    /// An overhung force: the tip deflects F a² (L + a) / (3 E I).
    #[test]
    fn overhang() {
        let s = plain(30.0, 300.0);
        let loads = ShaftLoads {
            supports: vec![
                Support {
                    z: 0.0,
                    kind: SupportKind::Fixed,
                },
                Support {
                    z: 200.0,
                    kind: SupportKind::Free,
                },
            ],
            loads: vec![Load::Radial {
                z: 300.0,
                force: 1000.0,
                angle: 90f64.to_radians(),
            }],
            use_density: false,
            ..ShaftLoads::default()
        };
        let r = calculate(&s, &loads).unwrap();
        let i = PI * 30f64.powi(4) / 64.0;
        let want = 1000.0 * 100f64.powi(2) * 300.0 / (3.0 * 206_000.0 * i);
        let tip = *r.deflection.last().unwrap_or(&0.0);
        assert!(close(tip, want, 2e-3), "{tip} vs {want}");
        // The X plane took it all.
        assert!(r.reactions[1][1].abs() > 1000.0 && r.reactions[1][0].abs() < 1e-9);
    }

    /// Eq. 33 on a uniform shaft between end supports: the exact first
    /// frequency is π² √(E I / (m L⁴)); Rayleigh's static curve is a hair above.
    #[test]
    fn critical_speed_of_a_uniform_shaft() {
        let s = plain(20.0, 1000.0);
        let loads = ShaftLoads {
            supports: vec![
                Support {
                    z: 0.0,
                    kind: SupportKind::Fixed,
                },
                Support {
                    z: 1000.0,
                    kind: SupportKind::Free,
                },
            ],
            ..ShaftLoads::default()
        };
        let r = calculate(&s, &loads).unwrap();
        let i = PI * 20f64.powi(4) / 64.0;
        // Mass per mm in N·s²/mm² (kg/mm → t/mm: /1000).
        let mu = 7850.0e-9 * PI / 4.0 * 400.0 / 1000.0;
        let w = PI * PI * (206_000.0 * i / (mu * 1e12)).sqrt();
        let rpm = w * 60.0 / (2.0 * PI);
        let got = r.critical_rpm.unwrap();
        assert!(got >= rpm * 0.999 && got < rpm * 1.003, "{got} vs {rpm}");
        assert!(close(r.mass, 7850.0 * PI / 4.0 * 0.02 * 0.02 * 1.0, 1e-6));
    }

    fn supports(z: &[f64]) -> Vec<Support> {
        z.iter()
            .map(|&z| Support {
                z,
                kind: SupportKind::Free,
            })
            .collect()
    }

    /// A continuous beam on three equal spans' supports: two spans L under a
    /// uniform load q carry 3/8 qL at the ends and 10/8 qL in the middle; two
    /// spans with a central force P each, 5/16 P and 22/16 P (the classical
    /// results of the three-moment equation).
    #[test]
    fn three_supports() {
        let s = plain(40.0, 2000.0);
        let q = 2.0;
        let loads = ShaftLoads {
            supports: supports(&[0.0, 1000.0, 2000.0]),
            loads: vec![Load::Continuous {
                z0: 0.0,
                z1: 2000.0,
                q,
                angle: 0.0,
            }],
            use_density: false,
            ..ShaftLoads::default()
        };
        let r = calculate(&s, &loads).unwrap();
        let ql = q * 1000.0;
        assert!(
            close(-r.reactions[0][0], 3.0 / 8.0 * ql, 2e-3),
            "{:?}",
            r.reactions
        );
        assert!(close(-r.reactions[1][0], 10.0 / 8.0 * ql, 2e-3));
        assert!(close(-r.reactions[2][0], 3.0 / 8.0 * ql, 2e-3));
        // Over the middle support M = qL² / 8.
        assert!(close(
            ShaftResults::max_abs(&r.moment),
            ql * 1000.0 / 8.0,
            3e-3
        ));
        let p = 1000.0;
        // The middle support listed first: the order of the list does not matter.
        let loads = ShaftLoads {
            supports: supports(&[1000.0, 0.0, 2000.0]),
            loads: vec![
                Load::Radial {
                    z: 500.0,
                    force: p,
                    angle: 90f64.to_radians(),
                },
                Load::Radial {
                    z: 1500.0,
                    force: p,
                    angle: 90f64.to_radians(),
                },
            ],
            use_density: false,
            ..ShaftLoads::default()
        };
        let r = calculate(&s, &loads).unwrap();
        assert!(
            close(-r.reactions[0][1], 22.0 / 16.0 * p, 1e-3),
            "{:?}",
            r.reactions
        );
        assert!(close(-r.reactions[1][1], 5.0 / 16.0 * p, 1e-3));
        assert!(close(-r.reactions[2][1], 5.0 / 16.0 * p, 1e-3));
        // Four supports: the reactions still sum to the load.
        let loads = ShaftLoads {
            supports: supports(&[0.0, 600.0, 1300.0, 2000.0]),
            ..loads
        };
        let r = calculate(&s, &loads).unwrap();
        let sum: f64 = r.reactions.iter().map(|x| x[1]).sum();
        assert!(close(-sum, 2.0 * p, 1e-9));
        for k in [1, 2] {
            let z = loads.supports[k].z;
            let i = r.z.iter().position(|&x| (x - z).abs() < 1e-9).unwrap();
            assert!(r.deflection[i].abs() < 1e-9, "{}", r.deflection[i]);
        }
    }

    /// A bending moment C at the end of a simply supported shaft: the
    /// reactions are ±C / L, the moment grows linearly to C, the slope at the
    /// far support is C L / (6 E I) and the middle deflects C L² / (16 E I).
    #[test]
    fn bending_moment_load() {
        let s = plain(30.0, 600.0);
        let c = 50e3;
        let loads = ShaftLoads {
            supports: supports(&[0.0, 600.0]),
            loads: vec![Load::Moment {
                z: 600.0,
                moment: c,
                angle: 0.0,
            }],
            use_density: false,
            ..ShaftLoads::default()
        };
        // A moment at the very end acts left of nothing: put it just inside.
        let mut l2 = loads.clone();
        l2.loads = vec![Load::Moment {
            z: 600.0 - 1e-6,
            moment: c,
            angle: 0.0,
        }];
        let r = calculate(&s, &l2).unwrap();
        assert!(
            close(r.reactions[0][0].abs(), c / 600.0, 1e-6),
            "{:?}",
            r.reactions
        );
        assert!(close(r.reactions[0][0], -r.reactions[1][0], 1e-9));
        assert!(close(ShaftResults::max_abs(&r.moment), c, 1e-3));
        let i = PI * 30f64.powi(4) / 64.0;
        let want = c * 600f64.powi(2) / (16.0 * 206_000.0 * i);
        let mid = r.z.iter().position(|&z| (z - 300.0).abs() < 1e-9).unwrap();
        assert!(
            close(r.deflection[mid], want, 2e-3),
            "{} vs {want}",
            r.deflection[mid]
        );
        assert!(calculate(&s, &loads).is_ok());
    }

    #[test]
    fn torques_and_errors() {
        let s = plain(40.0, 400.0);
        let two = vec![
            Support {
                z: 50.0,
                kind: SupportKind::Fixed,
            },
            Support {
                z: 350.0,
                kind: SupportKind::Free,
            },
        ];
        let loads = ShaftLoads {
            supports: two.clone(),
            loads: vec![
                Load::Torque {
                    z: 0.0,
                    torque: 200e3,
                },
                Load::Torque {
                    z: 400.0,
                    torque: -200e3,
                },
                Load::Axial {
                    z: 400.0,
                    force: -1000.0,
                },
            ],
            use_density: false,
            ..ShaftLoads::default()
        };
        let r = calculate(&s, &loads).unwrap();
        let tau = 16.0 * 200e3 / (PI * 64_000.0);
        assert!(close(ShaftResults::max_abs(&r.torsion_stress), tau, 1e-9));
        assert!(close(
            r.twist,
            twist_degrees(200e3, 400.0, 40.0, 79_300.0),
            0.01
        ));
        assert!(close(r.reactions[0][2], 1000.0, 1e-9));
        let (fs, _) = r.min_safety();
        assert!(fs > 1.0 && fs.is_finite());
        let mut bad = loads.clone();
        bad.loads.pop();
        bad.loads.pop();
        assert!(calculate(&s, &bad).unwrap_err().contains("sum to zero"));
        let mut one = loads;
        one.supports.pop();
        assert!(calculate(&s, &one).is_err());
    }
}
