//! Parallel keys (spec 6.2-design-calculators §3.3): the DIN 6885 form A size
//! for a shaft diameter and the key's check under a torque.
//!
//! Sizes: the widely published DIN 6885-1 table (shaft diameter over / up to,
//! width b × height h, groove depths t1 in the shaft and t2 in the hub).
//! Check: D. S. Kimball, J. H. Barr, *Elements of Machine Design*, Wiley 1909
//! (public domain), Chapter VIII §69 "Stresses in Sunk Keys": the force at
//! the shaft's surface P = 2T / d, the shear P = τ·l·b (eq. 1) and the crushing
//! P = p·l·t/2 (eq. 3, the key bearing on half its thickness). For a DIN key
//! the bearing height is the part standing in the hub, h − t1.

/// One row of DIN 6885 form A.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KeySize {
    /// Shaft diameter above which the row applies, mm.
    pub over: f64,
    /// Shaft diameter up to which it applies, mm.
    pub to: f64,
    /// Width b.
    pub b: f64,
    /// Height h.
    pub h: f64,
    /// Groove depth in the shaft t1.
    pub t1: f64,
    /// Groove depth in the hub t2.
    pub t2: f64,
}

const fn row(over: f64, to: f64, b: f64, h: f64, t1: f64, t2: f64) -> KeySize {
    KeySize {
        over,
        to,
        b,
        h,
        t1,
        t2,
    }
}

/// DIN 6885-1 form A, shafts of 6 … 230 mm.
pub const DIN_6885_A: &[KeySize] = &[
    row(6.0, 8.0, 2.0, 2.0, 1.2, 1.0),
    row(8.0, 10.0, 3.0, 3.0, 1.8, 1.4),
    row(10.0, 12.0, 4.0, 4.0, 2.5, 1.8),
    row(12.0, 17.0, 5.0, 5.0, 3.0, 2.3),
    row(17.0, 22.0, 6.0, 6.0, 3.5, 2.8),
    row(22.0, 30.0, 8.0, 7.0, 4.0, 3.3),
    row(30.0, 38.0, 10.0, 8.0, 5.0, 3.3),
    row(38.0, 44.0, 12.0, 8.0, 5.0, 3.3),
    row(44.0, 50.0, 14.0, 9.0, 5.5, 3.8),
    row(50.0, 58.0, 16.0, 10.0, 6.0, 4.3),
    row(58.0, 65.0, 18.0, 11.0, 7.0, 4.4),
    row(65.0, 75.0, 20.0, 12.0, 7.5, 4.9),
    row(75.0, 85.0, 22.0, 14.0, 9.0, 5.4),
    row(85.0, 95.0, 25.0, 14.0, 9.0, 5.4),
    row(95.0, 110.0, 28.0, 16.0, 10.0, 6.4),
    row(110.0, 130.0, 32.0, 18.0, 11.0, 7.4),
    row(130.0, 150.0, 36.0, 20.0, 12.0, 8.4),
    row(150.0, 170.0, 40.0, 22.0, 13.0, 9.4),
    row(170.0, 200.0, 45.0, 25.0, 15.0, 10.4),
    row(200.0, 230.0, 50.0, 28.0, 17.0, 11.4),
];

/// The standard key lengths, mm.
pub const LENGTHS: &[f64] = &[
    6.0, 8.0, 10.0, 12.0, 14.0, 16.0, 18.0, 20.0, 22.0, 25.0, 28.0, 32.0, 36.0, 40.0, 45.0, 50.0,
    56.0, 63.0, 70.0, 80.0, 90.0, 100.0, 110.0, 125.0, 140.0, 160.0, 180.0, 200.0, 220.0, 250.0,
    280.0, 320.0, 360.0, 400.0,
];

/// The key for a shaft of diameter `d`.
pub fn size_for(d: f64) -> Result<KeySize, String> {
    DIN_6885_A
        .iter()
        .find(|r| d > r.over && d <= r.to)
        .copied()
        .ok_or_else(|| format!("no DIN 6885 key for a Ø{d} shaft (6 … 230 mm)"))
}

/// The longest standard length not above `l`.
pub fn standard_length(l: f64) -> Option<f64> {
    LENGTHS.iter().rev().find(|&&s| s <= l + 1e-9).copied()
}

/// The ends of the key.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Ends {
    /// Form A: both ends rounded; they carry nothing (bearing length l − b).
    #[default]
    Rounded,
    /// Form B: square ends (bearing length l).
    Square,
}

/// The result of a key check.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KeyCheck {
    /// Force at the shaft surface P = 2T / d, N.
    pub force: f64,
    /// Length that carries load, mm.
    pub bearing_length: f64,
    /// Shear stress in the key, MPa (Kimball & Barr eq. 1).
    pub shear: f64,
    /// Pressure on the hub's side of the groove, MPa (eq. 3 with h − t1).
    pub pressure: f64,
    /// Allowed shear / shear.
    pub shear_safety: f64,
    /// Allowed pressure / pressure.
    pub pressure_safety: f64,
}

/// Checks `keys` keys of size `k` and length `l` on a shaft of diameter `d`
/// under torque `t` (N·mm) against the allowed shear and pressure (MPa). Two
/// keys are taken to carry 0.75 of twice one key's share (they never share
/// evenly).
#[allow(clippy::too_many_arguments)]
pub fn check(
    k: &KeySize,
    d: f64,
    l: f64,
    ends: Ends,
    keys: u32,
    t: f64,
    allowed_shear: f64,
    allowed_pressure: f64,
) -> Result<KeyCheck, String> {
    let bearing = match ends {
        Ends::Rounded => l - k.b,
        Ends::Square => l,
    };
    if bearing.is_nan() || bearing <= 0.0 {
        return Err("the key is shorter than it is wide".into());
    }
    if (d.is_nan() || d <= 0.0) || keys == 0 {
        return Err("no shaft or no key".into());
    }
    let share = if keys > 1 {
        0.75 * f64::from(keys)
    } else {
        1.0
    };
    let force = 2.0 * t / d;
    let shear = force / (k.b * bearing * share);
    let pressure = force / ((k.h - k.t1) * bearing * share);
    Ok(KeyCheck {
        force,
        bearing_length: bearing,
        shear,
        pressure,
        shear_safety: allowed_shear / shear,
        pressure_safety: allowed_pressure / pressure,
    })
}

/// The shortest standard length whose pressure stays within `allowed_pressure`.
pub fn shortest_length(
    k: &KeySize,
    d: f64,
    ends: Ends,
    keys: u32,
    t: f64,
    allowed_pressure: f64,
) -> Option<f64> {
    LENGTHS.iter().copied().find(|&l| {
        check(k, d, l, ends, keys, t, f64::INFINITY, allowed_pressure)
            .is_ok_and(|c| c.pressure_safety >= 1.0)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_by_diameter() {
        let k = size_for(40.0).unwrap();
        assert_eq!((k.b, k.h, k.t1, k.t2), (12.0, 8.0, 5.0, 3.3));
        assert_eq!(size_for(30.0).unwrap().b, 8.0);
        assert_eq!(size_for(30.01).unwrap().b, 10.0);
        assert!(size_for(5.0).is_err() && size_for(240.0).is_err());
        // Every row continues the one before and h > t1.
        for w in DIN_6885_A.windows(2) {
            assert_eq!(w[0].to, w[1].over);
        }
        assert!(DIN_6885_A.iter().all(|r| r.h > r.t1 && r.b > 0.0));
        assert_eq!(standard_length(47.0), Some(45.0));
    }

    /// 400 N·m on a Ø40 shaft, a 12 × 8 key 50 long with rounded ends:
    /// P = 2·400 000 / 40 = 20 kN over 38 mm; τ = 20 000 / (12·38) = 43.9 MPa,
    /// p = 20 000 / (3·38) = 175.4 MPa.
    #[test]
    fn check_by_kimball_and_barr() {
        let k = size_for(40.0).unwrap();
        let c = check(&k, 40.0, 50.0, Ends::Rounded, 1, 400e3, 80.0, 200.0).unwrap();
        assert!((c.force - 20_000.0).abs() < 1e-9);
        assert_eq!(c.bearing_length, 38.0);
        assert!((c.shear - 20_000.0 / (12.0 * 38.0)).abs() < 1e-9);
        assert!((c.pressure - 20_000.0 / (3.0 * 38.0)).abs() < 1e-9);
        assert!(c.pressure_safety > 1.0 && c.pressure_safety < 1.2);
        // Two keys: 1.5 times one.
        let two = check(&k, 40.0, 50.0, Ends::Rounded, 2, 400e3, 80.0, 200.0).unwrap();
        assert!((two.pressure * 1.5 - c.pressure).abs() < 1e-9);
        // The shortest length that holds 200 MPa: l − 12 ≥ 20 000 / 600 = 33.3 → 50.
        assert_eq!(
            shortest_length(&k, 40.0, Ends::Rounded, 1, 400e3, 200.0),
            Some(50.0)
        );
        assert!(check(&k, 40.0, 10.0, Ends::Rounded, 1, 400e3, 80.0, 200.0).is_err());
    }
}
