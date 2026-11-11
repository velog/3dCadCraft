//! Spur and helical gear pairs (spec 6.2-design-calculators §3.4): geometry,
//! bending stress and surface-fatigue life.
//!
//! Source: J. J. Coy, D. P. Townsend, E. V. Zaretsky, *Gearing*, NASA Reference
//! Publication 1152, 1985 (public domain). Equation numbers below are that
//! report's. Profile shift (Inventor's *unit correction*) is not in RP-1152: its
//! relations follow from the involute geometry of §2.1 (the involute function
//! and the operating pressure angle of a shifted pair); ISO 21771 names the same
//! quantities. Lengths in mm, angles in radians, forces in N, torques in N·mm.

use std::f64::consts::PI;

/// A pair of external gears.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GearPair {
    /// Normal module, mm (a spur gear: the module).
    pub module: f64,
    /// Normal pressure angle.
    pub pressure_angle: f64,
    /// Helix angle (0: spur).
    pub helix: f64,
    /// Teeth of gear 1 (the pinion) and gear 2.
    pub teeth: [u32; 2],
    /// Profile shifts (unit corrections) x1, x2.
    pub shift: [f64; 2],
    /// Face widths.
    pub face: [f64; 2],
    /// Addendum coefficient (1 for the standard rack).
    pub addendum: f64,
    /// Clearance coefficient (0.25 for the standard rack).
    pub clearance: f64,
}

impl Default for GearPair {
    fn default() -> GearPair {
        GearPair {
            module: 2.0,
            pressure_angle: 20f64.to_radians(),
            helix: 0.0,
            teeth: [20, 40],
            shift: [0.0, 0.0],
            face: [20.0, 20.0],
            addendum: 1.0,
            clearance: 0.25,
        }
    }
}

/// The pair's geometry.
#[derive(Clone, Debug, PartialEq)]
pub struct Geometry {
    /// Transverse module m / cos β.
    pub transverse_module: f64,
    /// Transverse pressure angle.
    pub transverse_pressure_angle: f64,
    /// Operating (working) pressure angle of the shifted pair.
    pub working_pressure_angle: f64,
    /// Pitch diameters d = z·mt (eq. 4: D = N / P).
    pub pitch: [f64; 2],
    /// Base diameters (eq. 3: pb = p cos φ).
    pub base: [f64; 2],
    /// Tip diameters.
    pub tip: [f64; 2],
    /// Root diameters.
    pub root: [f64; 2],
    /// Centre distance at zero backlash (eq. 6 for unshifted gears).
    pub center_distance: f64,
    /// Circular pitch p (eq. 1) and base pitch pb (eq. 2), transverse.
    pub circular_pitch: f64,
    /// Transverse base pitch.
    pub base_pitch: f64,
    /// Length of the path of contact Z (eq. 8).
    pub contact_length: f64,
    /// Transverse contact ratio Z / pb (eq. 11).
    pub contact_ratio: f64,
    /// Overlap (face) ratio b·sin β / (π m) of the narrower face.
    pub overlap_ratio: f64,
    /// Tip shortening k·m applied to both tips (shifted pairs).
    pub tip_shortening: f64,
    /// Warnings: interference (eq. 12), contact ratio below 1.2, pointed teeth.
    pub warnings: Vec<String>,
}

/// The involute function tan α − α.
pub fn inv(a: f64) -> f64 {
    a.tan() - a
}

/// The angle whose involute function is `v` (Newton from a cube-root start).
pub fn inv_inverse(v: f64) -> f64 {
    let mut a = (3.0 * v).cbrt();
    for _ in 0..50 {
        let f = inv(a) - v;
        let df = a.tan().powi(2);
        if df <= 0.0 {
            break;
        }
        let step = f / df;
        a -= step;
        if step.abs() < 1e-15 {
            break;
        }
    }
    a
}

impl GearPair {
    /// The geometry, or why the pair cannot exist.
    pub fn geometry(&self) -> Result<Geometry, String> {
        let m = self.module;
        let [z1, z2] = self.teeth.map(f64::from);
        if m.is_nan() || m <= 0.0 {
            return Err("the module must be positive".into());
        }
        if self.teeth.iter().any(|&z| z < 5) {
            return Err("a gear needs at least 5 teeth".into());
        }
        let beta = self.helix;
        if !(0.0..PI / 2.0 - 0.1).contains(&beta.abs()) {
            return Err("the helix angle must be below 84°".into());
        }
        let an = self.pressure_angle;
        let mt = m / beta.cos();
        let at = (an.tan() / beta.cos()).atan();
        let d = [z1 * mt, z2 * mt];
        let db = [d[0] * at.cos(), d[1] * at.cos()];
        let [x1, x2] = self.shift;
        // Shifted pair at zero backlash: inv αw = inv αt + 2 tan αn (x1 + x2) / (z1 + z2).
        let aw = inv_inverse(inv(at) + 2.0 * an.tan() * (x1 + x2) / (z1 + z2));
        if aw.is_nan() || aw <= 0.0 {
            return Err("the profile shifts are too negative".into());
        }
        let a0 = (d[0] + d[1]) / 2.0;
        let a = a0 * at.cos() / aw.cos();
        // Tip shortening keeps the standard clearance at the operating centre distance.
        let k = ((x1 + x2) - (a - a0) / m).max(0.0);
        let ha = self.addendum;
        let tip = [
            d[0] + 2.0 * m * (ha + x1 - k),
            d[1] + 2.0 * m * (ha + x2 - k),
        ];
        let root = [
            d[0] - 2.0 * m * (ha + self.clearance - x1),
            d[1] - 2.0 * m * (ha + self.clearance - x2),
        ];
        let p = PI * mt;
        let pb = p * at.cos();
        let ra = [tip[0] / 2.0, tip[1] / 2.0];
        let rb = [db[0] / 2.0, db[1] / 2.0];
        // Eq. 8 with the operating pitch circles and pressure angle.
        let zc = (ra[0].powi(2) - rb[0].powi(2)).max(0.0).sqrt()
            + (ra[1].powi(2) - rb[1].powi(2)).max(0.0).sqrt()
            - a * aw.sin();
        let mc = zc / pb;
        let overlap = self.face[0].min(self.face[1]) * beta.sin().abs() / (PI * m);
        let mut warnings = Vec::new();
        // Eq. 12: the largest tip radius of one gear that keeps clear of the other's base circle.
        for (i, j) in [(0usize, 1usize), (1, 0)] {
            let limit = (rb[i].powi(2) + (a * aw.sin()).powi(2)).sqrt();
            if ra[i] > limit + 1e-9 {
                warnings.push(format!(
                    "gear {}: its tip reaches below the base circle of gear {} (interference)",
                    i + 1,
                    j + 1
                ));
            }
        }
        if mc < 1.2 {
            warnings.push(format!("the contact ratio {mc:.2} is below 1.2"));
        }
        for i in 0..2 {
            if tip_thickness(d[i], db[i], tip[i], at, self.shift[i], m, self.teeth[i]) < 0.2 * m {
                warnings.push(format!("gear {}: the tooth tip is nearly pointed", i + 1));
            }
            if root[i] <= 0.0 {
                return Err(format!("gear {}: no material below the root", i + 1));
            }
        }
        Ok(Geometry {
            transverse_module: mt,
            transverse_pressure_angle: at,
            working_pressure_angle: aw,
            pitch: d,
            base: db,
            tip,
            root,
            center_distance: a,
            circular_pitch: p,
            base_pitch: pb,
            contact_length: zc,
            contact_ratio: mc,
            overlap_ratio: overlap,
            tip_shortening: k * m,
            warnings,
        })
    }

    /// The total unit correction x1 + x2 that gives centre distance `a`
    /// (Inventor's design guide *Total Unit Correction*).
    pub fn shift_sum_for(&self, a: f64) -> Result<f64, String> {
        let mt = self.module / self.helix.cos();
        let at = (self.pressure_angle.tan() / self.helix.cos()).atan();
        let [z1, z2] = self.teeth.map(f64::from);
        let a0 = (z1 + z2) * mt / 2.0;
        let c = a0 * at.cos() / a;
        if !(c > 0.0 && c < 1.0) {
            return Err(format!("no pair fits a centre distance of {a:.3} mm"));
        }
        let aw = c.acos();
        Ok((inv(aw) - inv(at)) * (z1 + z2) / (2.0 * self.pressure_angle.tan()))
    }
}

/// Transverse tooth thickness at the tip (the involute's thickness formula).
fn tip_thickness(d: f64, db: f64, da: f64, at: f64, x: f64, m: f64, z: u32) -> f64 {
    let s = PI * d / f64::from(z) / 2.0 + 2.0 * x * m * at.tan();
    let aa = (db / da).clamp(-1.0, 1.0).acos();
    da * (s / d + inv(at) - inv(aa))
}

/// The loads of the mesh for torque `t` on gear 1 (RP-1152 §5.1 example):
/// tangential Wt = T / rp and normal WN = T / rb.
pub fn tooth_loads(g: &Geometry, torque: f64) -> (f64, f64) {
    (2.0 * torque / g.pitch[0], 2.0 * torque / g.base[0])
}

/// Torque from power (W) and speed (rpm): T = P / ω, in N·mm.
pub fn torque_from_power(power_w: f64, rpm: f64) -> f64 {
    power_w / (rpm * 2.0 * PI / 60.0) * 1000.0
}

/// The factors of the modified Lewis (AGMA 218.01) bending stress, eq. 53.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BendingFactors {
    /// Geometry factor J (RP-1152 Fig. 27 for 20° spur gears).
    pub j: f64,
    /// Application factor Ka (Table 8).
    pub ka: f64,
    /// Load distribution factor Km (Table 9).
    pub km: f64,
    /// Dynamic factor Kv (Fig. 25), ≤ 1.
    pub kv: f64,
}

impl Default for BendingFactors {
    fn default() -> BendingFactors {
        BendingFactors {
            j: 0.35,
            ka: 1.0,
            km: 1.3,
            kv: 0.8,
        }
    }
}

/// Size factor, eq. 54: Ks = P^-0.25 for diametral pitch P < 16 in⁻¹, else 0.5.
/// P = 25.4 / m.
pub fn size_factor(module: f64) -> f64 {
    let p = 25.4 / module;
    if p < 16.0 { p.powf(-0.25) } else { 0.5 }
}

/// Bending stress number, eq. 53: Sb = (Wt / F)(P / J)(Ka Ks Km / Kv), with P = 1 / m
/// in metric units. MPa.
pub fn bending_stress(wt: f64, face: f64, module: f64, f: &BendingFactors) -> f64 {
    wt / face / module / f.j * f.ka * size_factor(module) * f.km / f.kv
}

/// Allowable bending stress, eq. 57: Sat·KL / (KT·KR).
pub fn allowable_bending(sat: f64, kl: f64, kt: f64, kr: f64) -> f64 {
    sat * kl / (kt * kr)
}

/// The surface-fatigue life of one tooth (RP-1152 §5.1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ToothLife {
    /// Roll angle from the base of the involute to the lowest point of contact, rad.
    pub eps_c: f64,
    /// Roll angle increment of double-pair contact (light load zone), rad.
    pub eps_l: f64,
    /// Roll angle increment of single-pair contact (heavy load zone), rad.
    pub eps_h: f64,
    /// Loaded profile length l, mm.
    pub loaded_length: f64,
    /// Curvature sum Σρ, eq. 69, 1/mm.
    pub curvature_sum: f64,
    /// 90 % reliability life of one tooth T10, eq. 68, millions of cycles.
    pub t10: f64,
}

/// Weibull slope of gear surface fatigue (RP-1152 §5.1).
pub const GEAR_WEIBULL: f64 = 2.5;

/// The constant of eq. 68 for newtons and millimetres (AISI 9310, NASA's tests).
pub const C10_N_MM: f64 = 6.44e9;

/// Single-tooth life of gear 1 under normal load `wn` (eq. 65–69). `face` is the
/// effective face width. The pair must have a contact ratio between 1 and 2.
/// A helical pair with an axial contact ratio above 2 takes the adjustments of
/// eq. 76–78 (`helical` true).
pub fn tooth_life(
    g: &Geometry,
    pair: &GearPair,
    wn: f64,
    face: f64,
    helical: bool,
) -> Result<ToothLife, String> {
    let rb = [g.base[0] / 2.0, g.base[1] / 2.0];
    let ra2 = g.tip[1] / 2.0;
    let phi = g.working_pressure_angle;
    let (z, pb) = (g.contact_length, g.base_pitch);
    if !(g.contact_ratio > 1.0 && g.contact_ratio < 2.0) {
        return Err(format!(
            "the life model needs a contact ratio between 1 and 2 (it is {:.2})",
            g.contact_ratio
        ));
    }
    // Operating pitch radii (equal to the pitch radii for unshifted gears).
    let c = g.center_distance;
    let [z1, z2] = pair.teeth.map(f64::from);
    let rw = [c * z1 / (z1 + z2), c * z2 / (z1 + z2)];
    let eps_c = ((rw[0] + rw[1]) * phi.sin() - (ra2 * ra2 - rb[1] * rb[1]).sqrt()) / rb[0];
    let eps_l = (z - pb) / rb[0];
    let eps_h = (2.0 * pb - z) / rb[0];
    let loaded = rb[0] * eps_h * (eps_c + eps_l + eps_h / 2.0);
    let mut sum = (1.0 / rw[0] + 1.0 / rw[1]) / phi.sin();
    let (mut w, mut f) = (wn, face);
    if helical {
        let psi = pair.helix;
        w = wn * pb / (0.95 * z * psi.cos());
        sum *= psi.cos();
        f = face / psi.cos();
    }
    let t10 = C10_N_MM * w.powf(-4.3) * f.powf(3.9) * sum.powf(-5.0) * loaded.powf(-0.4);
    Ok(ToothLife {
        eps_c,
        eps_l,
        eps_h,
        loaded_length: loaded,
        curvature_sum: sum,
        t10,
    })
}

/// A life at reliability `s` (0..1) from the 90 % life, eq. 67.
pub fn life_at(l10: f64, s: f64, weibull: f64) -> f64 {
    l10 * ((1.0 / s).ln() / (1.0f64 / 0.9).ln()).powf(1.0 / weibull)
}

/// Gear life from the tooth life, eq. 70 (simple gear, N teeth).
pub fn gear_life(t10: f64, teeth: u32) -> f64 {
    t10 * f64::from(teeth).powf(-1.0 / GEAR_WEIBULL)
}

/// Life of a system of components at reliability `s`, eq. 75: each component's
/// 90 % life and Weibull slope. Solved by bisection (eq. 73 / 74 when all
/// slopes are equal).
pub fn system_life(parts: &[(f64, f64)], s: f64) -> f64 {
    let target = (1.0 / s).ln() / (1.0f64 / 0.9).ln();
    let f = |l: f64| parts.iter().map(|&(li, e)| (l / li).powf(e)).sum::<f64>() - target;
    let (mut lo, mut hi) = (0.0, parts.iter().map(|p| p.0).fold(0.0, f64::max) * 10.0);
    for _ in 0..200 {
        let mid = (lo + hi) / 2.0;
        if f(mid) > 0.0 {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    (lo + hi) / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, rel: f64) -> bool {
        (a - b).abs() <= rel * b.abs()
    }

    /// The example of RP-1152 §5.1: two equal gears, 28 teeth, module 3.18
    /// (8 teeth per inch), 20°, face 9.52 mm, 149 kW at 10 000 rpm.
    fn book_pair() -> GearPair {
        GearPair {
            module: 25.4 / 8.0,
            teeth: [28, 28],
            face: [9.52, 9.52],
            ..GearPair::default()
        }
    }

    #[test]
    fn spur_life_example_of_rp1152() {
        let pair = book_pair();
        let g = pair.geometry().unwrap();
        assert!(close(g.pitch[0] / 2.0, 44.5, 0.002), "{}", g.pitch[0]);
        assert!(close(g.base[0] / 2.0, 41.8, 0.002));
        assert!(close(g.tip[0] / 2.0, 47.6, 0.002));
        assert!(close(g.base_pitch, 9.38, 0.002));
        // The path of contact is a small difference of large numbers: the book's
        // radii rounded to 0.1 mm give 15.10 mm and 1.61, the exact ones 15.35 and 1.64.
        assert!(close(g.contact_length, 15.10, 0.02), "{}", g.contact_length);
        assert!(close(g.contact_ratio, 1.61, 0.02), "{}", g.contact_ratio);
        let t = torque_from_power(149e3, 10_000.0);
        assert!(close(t, 142.3e3, 0.002), "{t}");
        let (_, wn) = tooth_loads(&g, t);
        assert!(close(wn, 3400.0, 0.003), "{wn}");
        // The life equations with the book's rounded numbers.
        let g = book_geometry(&g);
        let life = tooth_life(&g, &pair, 3400.0, 9.52, false).unwrap();
        assert!(close(life.eps_c, 0.1835, 0.005), "{}", life.eps_c);
        assert!(close(life.eps_l, 0.1368, 0.005), "{}", life.eps_l);
        assert!(close(life.eps_h, 0.0876, 0.01), "{}", life.eps_h);
        assert!(
            close(life.loaded_length, 1.333, 0.005),
            "{}",
            life.loaded_length
        );
        assert!(close(life.curvature_sum, 0.1314, 0.003));
        // 627 million cycles, 1045 h at 10 000 rpm, 408 h at 99 %.
        assert!(close(life.t10, 627.0, 0.01), "{}", life.t10);
        let hours = 627e6 / 10_000.0 / 60.0;
        assert!(close(hours, 1045.0, 0.001));
        assert!(close(life_at(1045.0, 0.99, GEAR_WEIBULL), 408.0, 0.003));
    }

    /// The example's geometry as the book rounds it (rp 44.5, rb 41.8, ra 47.6,
    /// Z 15.10, pb 9.38 mm).
    fn book_geometry(g: &Geometry) -> Geometry {
        Geometry {
            pitch: [89.0, 89.0],
            base: [83.6, 83.6],
            tip: [95.2, 95.2],
            center_distance: 89.0,
            contact_length: 15.10,
            base_pitch: 9.38,
            contact_ratio: 15.10 / 9.38,
            ..g.clone()
        }
    }

    /// RP-1152 §5.4: the same pair as a 45° helical gear with a 25.4 mm face.
    #[test]
    fn helical_life_example_of_rp1152() {
        let pair = GearPair {
            helix: 45f64.to_radians(),
            face: [25.4, 25.4],
            ..book_pair()
        };
        // The book keeps the transverse plane of the spur example.
        let g = book_geometry(&book_pair().geometry().unwrap());
        let life = tooth_life(&g, &pair, 3400.0, 25.4, true).unwrap();
        assert!(close(life.curvature_sum, 0.0929, 0.003));
        // 8.85 × 10^11 cycles = 885 000 million.
        assert!(close(life.t10, 8.85e5, 0.02), "{}", life.t10);
    }

    /// RP-1152 §5.3: four bearings and two gears, 249 h at 95 %.
    #[test]
    fn system_life_example_of_rp1152() {
        let parts = [
            (1200.0, 10.0 / 9.0),
            (1500.0, 10.0 / 9.0),
            (1300.0, 1.5),
            (1700.0, 1.5),
            (1000.0, 2.5),
            (1900.0, 2.5),
        ];
        let l = system_life(&parts, 0.95);
        assert!(close(l, 249.0, 0.01), "{l}");
    }

    /// Unshifted gears: the centre distance is (d1 + d2) / 2 (eq. 6), and the
    /// shift sum for it is zero; a shifted pair moves apart by about (x1 + x2)·m.
    #[test]
    fn shifts_and_centre_distance() {
        let p = GearPair::default();
        let g = p.geometry().unwrap();
        assert!(close(g.center_distance, 60.0, 1e-9));
        assert!(p.shift_sum_for(60.0).unwrap().abs() < 1e-9);
        let s = GearPair {
            shift: [0.3, 0.2],
            ..p
        };
        let gs = s.geometry().unwrap();
        assert!(gs.center_distance > 60.0 && gs.center_distance < 61.0);
        let back = p.shift_sum_for(gs.center_distance).unwrap();
        assert!((back - 0.5).abs() < 1e-9, "{back}");
        // A helical pair: mt = m / cos β.
        let h = GearPair {
            helix: 15f64.to_radians(),
            ..p
        };
        let gh = h.geometry().unwrap();
        assert!(close(gh.pitch[0], 40.0 / 15f64.to_radians().cos(), 1e-12));
        assert!(gh.overlap_ratio > 0.8);
    }

    /// Few teeth cut below the base circle: 12 teeth against 60 interferes.
    #[test]
    fn interference_is_reported() {
        let p = GearPair {
            teeth: [12, 60],
            ..GearPair::default()
        };
        let g = p.geometry().unwrap();
        assert!(g.warnings.iter().any(|w| w.contains("interference")));
        assert!(GearPair::default().geometry().unwrap().warnings.is_empty());
    }

    #[test]
    fn bending_stress_by_eq_53() {
        // Ks: P = 25.4 / 2 = 12.7 < 16.
        assert!(close(size_factor(2.0), 12.7f64.powf(-0.25), 1e-12));
        assert_eq!(size_factor(1.0), 0.5);
        let f = BendingFactors {
            j: 0.4,
            ka: 1.25,
            km: 1.3,
            kv: 0.8,
        };
        let s = bending_stress(2000.0, 20.0, 2.0, &f);
        let want = 2000.0 / 20.0 / 2.0 / 0.4 * 1.25 * size_factor(2.0) * 1.3 / 0.8;
        assert!(close(s, want, 1e-12));
        assert!(close(
            allowable_bending(300.0, 1.0, 1.0, 1.25),
            240.0,
            1e-12
        ));
    }
}
