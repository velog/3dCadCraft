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
    /// Tip radius coefficient of the generating rack (0.38: the largest round
    /// that fits a 0.25 clearance at 20°, ISO 53 profile A).
    pub tip_radius: f64,
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
            tip_radius: 0.38,
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

/// The geometry factor J of one gear and how it came out (RP-1152 §4.1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GeometryFactor {
    /// J = Y / Kf (load sharing 1: the load at the highest point of
    /// single-tooth contact, or at the tip, carried by one tooth).
    pub j: f64,
    /// The form factor Y of the Lewis parabola, with the load's radial
    /// component.
    pub y: f64,
    /// Stress concentration Kf at the fillet (Dolan and Broghamer).
    pub kf: f64,
    /// Tooth thickness at the critical section, mm.
    pub thickness: f64,
    /// Height of the load line's crossing with the tooth centre line above the
    /// critical section, mm.
    pub height: f64,
    /// Smallest radius of curvature of the fillet, mm.
    pub fillet_radius: f64,
    /// Radius of the load point, mm.
    pub load_radius: f64,
}

/// The geometry factor J of gear `i` (0 or 1) of the pair, after RP-1152 §4.1:
/// the form factor Y "from a geometrical layout of the Lewis parabola within
/// the gear tooth outline (fig. 23)", the load at the highest point of
/// single-tooth contact (or at the tip, `at_tip`), and the stress
/// concentration of Dolan and Broghamer. We lay the tooth out exactly instead
/// of reading Fig. 27:
///
/// - the tooth is generated by the pair's rack (addendum + clearance deep,
///   with its tip round): the involute above the form circle, the fillet as
///   the envelope of the rack's tip round rolled with the gear (undercut
///   included);
/// - the load acts along the line of action at the load point; the Lewis
///   parabola has its apex where that line crosses the tooth's centre line
///   and touches the fillet at the critical section (thickness s, height h);
/// - Y = 1 / ((cos φL / cos φ)(6 h / s² − tan φL / s)) in module units: the
///   bending of the load's tangential component less the compression of its
///   radial one (eq. 50 with the radial component the Lewis formula leaves out);
/// - Kf = H + (s / ρ)^L (s / h)^M with Dolan and Broghamer's constants
///   (14.5°: 0.22, 0.20, 0.40; 20°: 0.18, 0.15, 0.45; linear in the pressure
///   angle between and beyond) and ρ the fillet's smallest radius of curvature.
///
/// Spur gears only (helical gears need the load sharing along the face).
pub fn geometry_factor(
    pair: &GearPair,
    g: &Geometry,
    i: usize,
    at_tip: bool,
) -> Result<GeometryFactor, String> {
    if pair.helix.abs() > 1e-9 {
        return Err("J is computed for spur gears only (helical: give J)".into());
    }
    let i = i.min(1);
    let j = 1 - i;
    let m = pair.module;
    let a = pair.pressure_angle;
    let t = Tooth::new(pair, g, i)?;
    let (rb, ra, r_form) = (t.rb, t.ra, t.r_form);
    // The load point.
    let r_load = if at_tip {
        ra
    } else {
        let rbj = f64::from(pair.teeth[j]) / 2.0 * a.cos();
        let raj = g.tip[j] / 2.0 / m;
        let c = g.center_distance / m;
        let lowest = c * g.working_pressure_angle.sin() - (raj * raj - rbj * rbj).max(0.0).sqrt();
        let xi = lowest + PI * a.cos();
        (rb * rb + xi * xi).sqrt().min(ra)
    };
    if r_load <= r_form {
        return Err("the load point lies below the involute".into());
    }
    let al = (rb / r_load).acos();
    let psi_l = t.psi_inv(r_load);
    let phi_l = al - psi_l;
    let y_apex = r_load * psi_l.cos() - r_load * psi_l.sin() * phi_l.tan();
    // The tooth's side from the root to the load point: (half thickness,
    // height above the centre, radius).
    let side: Vec<(f64, f64, f64)> = t
        .side(r_load, 800)
        .into_iter()
        .map(|(rr, psi)| (rr * psi.sin(), rr * psi.cos(), rr))
        .collect();
    // The Lewis parabola touches the fillet where x² / (apex − y) is least.
    let crit = side
        .iter()
        .enumerate()
        .filter(|(_, p)| p.2 <= r_form && p.1 < y_apex && p.0 > 0.0)
        .min_by(|a, b| {
            let qa = a.1.0 * a.1.0 / (y_apex - a.1.1);
            let qb = b.1.0 * b.1.0 / (y_apex - b.1.1);
            qa.total_cmp(&qb)
        })
        .map(|(k, _)| k)
        .ok_or("no fillet below the load point")?;
    let (xf, yf, _) = side[crit];
    let (sf, hf) = (2.0 * xf, y_apex - yf);
    // The fillet's smallest radius of curvature (circles through samples).
    let fillet: Vec<(f64, f64)> = side
        .iter()
        .filter(|p| p.2 <= r_form)
        .map(|p| (p.0, p.1))
        .collect();
    let rho = (4..fillet.len().saturating_sub(4))
        .map(|k| circle_radius(fillet[k - 4], fillet[k], fillet[k + 4]))
        .fold(f64::INFINITY, f64::min);
    if !(rho.is_finite() && sf > 0.0 && hf > 0.0) {
        return Err("the tooth's fillet could not be laid out".into());
    }
    let y = 1.0 / ((phi_l.cos() / a.cos()) * (6.0 * hf / (sf * sf) - phi_l.tan() / sf));
    let t = (a.to_degrees() - 14.5) / 5.5;
    let (h, l, mm) = (0.22 - 0.04 * t, 0.20 - 0.05 * t, 0.40 + 0.05 * t);
    let kf = h + (sf / rho).powf(l) * (sf / hf).powf(mm);
    Ok(GeometryFactor {
        j: y / kf,
        y,
        kf,
        thickness: sf * m,
        height: hf * m,
        fillet_radius: rho * m,
        load_radius: r_load * m,
    })
}

/// A gear's tooth as its rack generates it, in module units: the involute
/// above the form circle, the fillet as the envelope of the rack's tip round
/// rolled with the gear (undercut included).
struct Tooth {
    z: f64,
    rb: f64,
    ra: f64,
    root: f64,
    r_form: f64,
    s0: f64,
    a: f64,
    /// The tip round's envelope as (radius, angle from the centre line of the
    /// space it cuts, towards the tooth).
    env: Vec<(f64, f64)>,
}

impl Tooth {
    fn new(pair: &GearPair, g: &Geometry, i: usize) -> Result<Tooth, String> {
        let m = pair.module;
        let a = pair.pressure_angle;
        let z = f64::from(pair.teeth[i]);
        let x = pair.shift[i];
        let r = z / 2.0;
        let rb = r * a.cos();
        let ha0 = pair.addendum + pair.clearance;
        let rho0 = pair.tip_radius;
        let hc = ha0 - rho0;
        // The tip round's centre, measured from the rack tooth's centre line.
        let uc = PI / 4.0 - hc * a.tan() - rho0 / a.cos();
        if !(rho0 >= 0.0 && uc >= -1e-9 && hc > 0.0) {
            return Err("the rack's tip round does not fit its tooth".into());
        }
        // Where the rack's straight flank ends: the involute starts there.
        let h_form = hc + rho0 * a.sin();
        let lf = r * a.sin() - (h_form - x) / a.sin();
        let r_form = if lf > 0.0 {
            (rb * rb + lf * lf).sqrt()
        } else {
            rb
        };
        // The tip round's centre in the gear's frame as the rack rolls on the
        // pitch circle (the gear turned back by the roll angle).
        let centre = |ph: f64| -> (f64, f64) {
            let (xw, yw) = (uc + r * ph, r + x - hc);
            (xw * ph.cos() - yw * ph.sin(), xw * ph.sin() + yw * ph.cos())
        };
        let (lo, hi) = ((-4.0 / r).max(-0.8), (8.0 / r).min(1.6));
        let samples = 4000u32;
        let env = (0..=samples)
            .filter_map(|k| {
                let ph = lo + (hi - lo) * f64::from(k) / f64::from(samples);
                let e = 1e-7;
                let (c, c1, c0) = (centre(ph), centre(ph + e), centre(ph - e));
                let (dx, dy) = (c1.0 - c0.0, c1.1 - c0.1);
                let n = dx.hypot(dy);
                (n > 1e-18).then(|| {
                    let (px, py) = (c.0 + rho0 * dy / n, c.1 - rho0 * dx / n);
                    (px.hypot(py), px.atan2(py))
                })
            })
            .collect();
        Ok(Tooth {
            z,
            rb,
            ra: g.tip[i] / 2.0 / m,
            root: r + x - ha0,
            r_form,
            s0: PI / 2.0 + 2.0 * x * a.tan(),
            a,
            env,
        })
    }

    /// Half the tooth's angle at radius `rr` on the involute.
    fn psi_inv(&self, rr: f64) -> f64 {
        let ar = (self.rb / rr).clamp(-1.0, 1.0).acos();
        self.s0 / self.z + inv(self.a) - inv(ar)
    }

    /// The deepest cut of the tip round at radius `rr`.
    fn fillet_angle(&self, rr: f64) -> Option<f64> {
        let mut best: Option<f64> = None;
        for w in self.env.windows(2) {
            let ((r0, t0), (r1, t1)) = (w[0], w[1]);
            if (r0 - rr) * (r1 - rr) <= 0.0 && (r1 - r0).abs() > 1e-15 {
                let t = t0 + (t1 - t0) * (rr - r0) / (r1 - r0);
                best = Some(best.map_or(t, |b: f64| b.max(t)));
            }
        }
        best
    }

    /// The side from the root to `top`: (radius, half the tooth's angle).
    fn side(&self, top: f64, steps: u32) -> Vec<(f64, f64)> {
        let mut out = Vec::with_capacity(steps as usize + 1);
        for k in 0..=steps {
            let rr = self.root + 1e-7 + (top - self.root) * f64::from(k) / f64::from(steps);
            let mut cut = self.fillet_angle(rr);
            if rr >= self.r_form && rr >= self.rb {
                let t = PI / self.z - self.psi_inv(rr);
                cut = Some(cut.map_or(t, |c| c.max(t)));
            }
            if let Some(t) = cut {
                out.push((rr, PI / self.z - t));
            }
        }
        out
    }
}

/// A tooth side's samples (radius, half angle) with the root and form radii.
pub type ToothSide = (Vec<(f64, f64)>, f64, f64);

/// The outline of gear `i`'s tooth side from the root circle to the tip, as
/// (radius mm, half the tooth's angle), `steps` + 1 samples; the root radius
/// and the form radius where the fillet meets the involute (mm). A tooth
/// centred on angle c has its sides at c ± angle.
pub fn tooth_side(
    pair: &GearPair,
    g: &Geometry,
    i: usize,
    steps: u32,
) -> Result<ToothSide, String> {
    let t = Tooth::new(pair, g, i.min(1))?;
    let m = pair.module;
    let side = t
        .side(t.ra, steps)
        .into_iter()
        .map(|(r, psi)| (r * m, psi))
        .collect();
    Ok((side, t.root * m, t.r_form * m))
}

/// The radius of the circle through three points (∞ when in line).
fn circle_radius(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> f64 {
    let ab = (a.0 - b.0).hypot(a.1 - b.1);
    let bc = (b.0 - c.0).hypot(b.1 - c.1);
    let ca = (c.0 - a.0).hypot(c.1 - a.1);
    let cross = ((b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)).abs();
    if cross < 1e-15 {
        f64::INFINITY
    } else {
        ab * bc * ca / (2.0 * cross)
    }
}

/// The dynamic factor Kv of RP-1152 Fig. 25 at pitch-line velocity `v` (m/s)
/// for transmission accuracy level `level` (the AGMA accuracy class): the
/// figure's curves are Kv = (A / (A + √(200 v)))^B with B = (12 − Qv)^(2/3) / 4
/// and A = 50 + 56 (1 − B), and Kv = 50 / (50 + √(200 v)) for level 5 and
/// below (checked against points read off the figure, within 0.02).
pub fn dynamic_factor(level: u32, v: f64) -> f64 {
    let v = v.max(0.0);
    if level <= 5 {
        return 50.0 / (50.0 + (200.0 * v).sqrt());
    }
    let b = 0.25 * (12.0 - f64::from(level.min(11))).powf(2.0 / 3.0);
    let a = 50.0 + 56.0 * (1.0 - b);
    (a / (a + (200.0 * v).sqrt())).powf(b)
}

/// Pitch-line velocity, m/s, of a pitch diameter `d` (mm) at `rpm`.
pub fn pitch_line_velocity(d: f64, rpm: f64) -> f64 {
    PI * d * rpm / 60_000.0
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

    /// RP-1152 Fig. 27 (20°, standard addendum, generating rack 1.4 deep with
    /// a 0.35 tip round, as the figure draws it): J read off the figure at
    /// the load's highest point of single-tooth contact and at the tip. The
    /// exact layout comes out within 5 % of the curves.
    #[test]
    fn geometry_factor_against_fig_27() {
        let j = |z: u32, mate: u32, tip: bool| {
            let p = GearPair {
                module: 1.0,
                teeth: [z, mate],
                clearance: 0.4,
                tip_radius: 0.35,
                ..GearPair::default()
            };
            let g = p.geometry().unwrap();
            geometry_factor(&p, &g, 0, tip).unwrap().j
        };
        // (teeth, mating teeth, at the tip, read off the figure)
        for (z, mate, tip, want) in [
            (20, 1000, false, 0.347),
            (20, 17, false, 0.313),
            (40, 1000, false, 0.445),
            (40, 17, false, 0.379),
            (20, 20, true, 0.238),
            (24, 24, true, 0.249),
            (40, 40, true, 0.272),
            (275, 275, true, 0.308),
        ] {
            let got = j(z, mate, tip);
            assert!(
                close(got, want, 0.05),
                "{z}/{mate} tip {tip}: {got} vs {want}"
            );
        }
        // More teeth, a larger mate: a larger J; the tip load the smallest.
        assert!(j(30, 30, false) > j(20, 20, false));
        assert!(j(20, 1000, false) > j(20, 17, false));
        assert!(j(20, 20, true) < j(20, 20, false));
        // Undercut teeth still lay out, and are weaker.
        assert!(j(12, 12, false) < j(17, 17, false));
        // Module scales the lengths, not J.
        let p = GearPair {
            module: 3.0,
            ..GearPair::default()
        };
        let g = p.geometry().unwrap();
        let f = geometry_factor(&p, &g, 0, false).unwrap();
        let p1 = GearPair { module: 1.0, ..p };
        let f1 = geometry_factor(&p1, &p1.geometry().unwrap(), 0, false).unwrap();
        assert!(close(f.j, f1.j, 1e-6) && close(f.thickness, 3.0 * f1.thickness, 1e-6));
        // A positive shift thickens the root.
        let ps = GearPair {
            shift: [0.4, -0.4],
            ..p1
        };
        let fs = geometry_factor(&ps, &ps.geometry().unwrap(), 0, false).unwrap();
        assert!(fs.thickness > f1.thickness && fs.j > f1.j, "{fs:?} {f1:?}");
    }

    /// RP-1152 Fig. 25: points read off the curves of accuracy levels 11 … 7
    /// and ≤ 5 at 5, 10 and 20 m/s.
    #[test]
    fn dynamic_factor_against_fig_25() {
        let read = [
            (5.0, [0.928, 0.881, 0.830, 0.782, 0.746, 0.698, 0.611]),
            (10.0, [0.896, 0.836, 0.777, 0.719, 0.672, 0.620, 0.518]),
            (20.0, [0.876, 0.800, 0.724, 0.659, 0.597, 0.0, 0.0]),
        ];
        for (v, row) in read {
            for (k, want) in row.iter().enumerate() {
                if *want == 0.0 {
                    continue;
                }
                let level = 11 - k as u32;
                let got = dynamic_factor(level, v);
                assert!(
                    (got - want).abs() < 0.02,
                    "level {level} at {v} m/s: {got} vs {want}"
                );
            }
        }
        assert!(close(dynamic_factor(8, 0.0), 1.0, 1e-12));
        assert!(close(pitch_line_velocity(100.0, 600.0), PI, 1e-12));
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
